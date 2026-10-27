//! New versions of Ezoo Download from GitHub, always on: right after the
//! program opens it looks for a newer release; when there is one it is
//! downloaded, checked (signature of the release + SHA-256) and installed silently, and the program
//! starts again by itself. While the program stays open it looks again every
//! few hours (then it waits until no download is running).
//! Help > Check for updates does the same at once in a window of its own
//! "checking…", "this is the newest version", or the download of
//! the new one with its progress, then its setup runs in front of the user.

use crate::i18n::{t, tf};
use crate::message;
use crate::{open_window, Ctx, Ui};
use slint::ComponentHandle;
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
fn apply(setup: PathBuf, sha256: String) {
    apply_as(setup, sha256, false)
}

fn apply_as(setup: PathBuf, sha256: String, visible: bool) {
    match selfupdate::run_installer(&setup, &sha256, visible) {
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
    let sha = rel.sha256.clone();
    let _ = slint::invoke_from_event_loop(move || apply(setup, sha));
    true
}

/// What the update window shows: written by the check / download (async
/// side), copied to the window by a timer on the UI thread.
#[derive(Clone, Default)]
struct Shown {
    state: i32,
    heading: String,
    detail: String,
    progress: f32,
    /// downloaded and checked: run this setup (path, sha256)
    install: Option<(PathBuf, String)>,
    done: bool,
}

thread_local! {
    static WINDOW_TIMER: slint::Timer = slint::Timer::default();
}

fn size_text(n: u64) -> String {
    let mb = n as f64 / (1024.0 * 1024.0);
    format!("{mb:.1} MB")
}

/// Help > Check for updates: look now instead of waiting for the next check,
/// in the update window.
pub fn check_now(ui: &Ui) {
    let Some(d) = ui.updater.upgrade() else { return };
    open_window!(d);
    let set = |state: i32, heading: String, detail: String| {
        d.set_state(state);
        d.set_heading(heading.into());
        d.set_detail(detail.into());
        d.set_progress(-1.0);
    };
    if BUSY.swap(true, Ordering::SeqCst) {
        set(0, t("A new version is being prepared. It is installed by itself when it is ready."), String::new());
        return;
    }
    set(0, t("Checking for updates…"), tf("This is version {}.", &[&selfupdate::current()]));
    let shared = Arc::new(std::sync::Mutex::new(Shown { state: 0, progress: -1.0, ..Default::default() }));
    {
        // UI thread: show what the async side wrote
        let (shared, weak) = (shared.clone(), d.as_weak());
        WINDOW_TIMER.with(|timer| {
            timer.start(slint::TimerMode::Repeated, Duration::from_millis(100), move || {
                let now = shared.lock().unwrap().clone();
                if let Some(d) = weak.upgrade() {
                    if now.state != 0 || !now.heading.is_empty() {
                        d.set_state(now.state);
                        d.set_heading(now.heading.clone().into());
                        d.set_detail(now.detail.clone().into());
                        d.set_progress(now.progress);
                    }
                }
                if let Some((setup, sha)) = shared.lock().unwrap().install.take() {
                    apply_as(setup, sha, true);
                }
                if now.done {
                    WINDOW_TIMER.with(|t| t.stop());
                }
            });
        });
    }
    let tell = move |shared: &std::sync::Mutex<Shown>, state: i32, heading: String, detail: String, progress: f32| {
        let mut s = shared.lock().unwrap();
        s.state = state;
        s.heading = heading;
        s.detail = detail;
        s.progress = progress;
    };
    let client = ui.ctx.mgr.client();
    ui.ctx.rt.spawn(async move {
        let finish = |shared: &std::sync::Mutex<Shown>| {
            BUSY.store(false, Ordering::SeqCst);
            shared.lock().unwrap().done = true;
        };
        let rel = match selfupdate::check(&client).await {
            Err(e) => {
                tell(&shared, 4, t("Could not check for updates"), format!("{e:#}"), -1.0);
                return finish(&shared);
            }
            Ok(None) => {
                tell(&shared, 1, t("Ezoo Download is up to date"), tf("Version {} is the newest version.", &[&selfupdate::current()]), -1.0);
                return finish(&shared);
            }
            Ok(Some(rel)) => rel,
        };
        if !selfupdate::installed() {
            tell(
                &shared,
                4,
                tf("Version {} is available", &[&rel.version]),
                t("This copy was not installed with the setup, so it cannot update itself."),
                -1.0,
            );
            return finish(&shared);
        }
        let heading = tf("Downloading version {}…", &[&rel.version]);
        tell(&shared, 2, heading.clone(), String::new(), -1.0);
        let (s2, h2) = (shared.clone(), heading.clone());
        let progress = move |done: u64, total: Option<u64>| {
            let (detail, p) = match total {
                Some(t) if t > 0 => (format!("{} / {}", size_text(done), size_text(t)), done as f32 / t as f32),
                _ => (size_text(done), -1.0),
            };
            tell(&s2, 2, h2.clone(), detail, p);
        };
        match selfupdate::download_with_progress(&client, &rel, &update_dir(), &progress).await {
            Ok(setup) => {
                tell(
                    &shared,
                    3,
                    tf("Installing version {}…", &[&rel.version]),
                    t("Ezoo Download closes now and opens again when it is installed."),
                    -1.0,
                );
                // a moment to read it, then the setup runs in front of the user
                tokio::time::sleep(Duration::from_millis(1200)).await;
                shared.lock().unwrap().install = Some((setup, rel.sha256.clone()));
                finish(&shared);
            }
            Err(e) => {
                tell(&shared, 4, t("Could not get the update"), format!("{e:#}"), -1.0);
                finish(&shared);
            }
        }
    });
}
