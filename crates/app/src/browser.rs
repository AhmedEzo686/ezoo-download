//! "Add Ezoo Download to my browser": opens the browser on the right page
//! (its store page once the extension is published there, otherwise its
//! extensions page with the folder path ready in the clipboard).

use crate::open_window;
use crate::*;
use crate::i18n::{t, tf};

/// Store pages of the published extension. Empty = not published yet:
/// then the extension is added from its folder (developer mode).
const STORE_CHROME: &str = "";
const STORE_EDGE: &str = "";
const STORE_FIREFOX: &str = "";
const STORE_OPERA: &str = "";
const STORE_BRAVE: &str = "";

/// The Firefox add-on signed by Mozilla (built by the release): installed
/// from this file it stays in Firefox for good.
const FIREFOX_XPI: &str = "ezoo-download-firefox.xpi";

#[derive(Clone)]
pub(crate) struct Browser {
    name: &'static str,
    exe: Option<PathBuf>,
    extensions_page: &'static str,
    store: &'static str,
    firefox: bool,
    tint: slint::Color,
}

fn env_path(var: &str, rest: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|v| PathBuf::from(v).join(rest))
}

fn first_existing(list: Vec<Option<PathBuf>>) -> Option<PathBuf> {
    list.into_iter().flatten().find(|p| p.exists())
}

fn detect() -> Vec<Browser> {
    let rgb = slint::Color::from_rgb_u8;
    vec![
        Browser {
            name: "Chrome",
            exe: first_existing(vec![
                env_path("ProgramFiles", r"Google\Chrome\Application\chrome.exe"),
                env_path("ProgramFiles(x86)", r"Google\Chrome\Application\chrome.exe"),
                env_path("LOCALAPPDATA", r"Google\Chrome\Application\chrome.exe"),
            ]),
            extensions_page: "chrome://extensions/",
            store: STORE_CHROME,
            firefox: false,
            tint: rgb(0x1f, 0xa4, 0x63),
        },
        Browser {
            name: "Edge",
            exe: first_existing(vec![
                env_path("ProgramFiles(x86)", r"Microsoft\Edge\Application\msedge.exe"),
                env_path("ProgramFiles", r"Microsoft\Edge\Application\msedge.exe"),
            ]),
            extensions_page: "edge://extensions/",
            store: STORE_EDGE,
            firefox: false,
            tint: rgb(0x0c, 0x8c, 0xd8),
        },
        Browser {
            name: "Firefox",
            exe: first_existing(vec![
                env_path("ProgramFiles", r"Mozilla Firefox\firefox.exe"),
                env_path("ProgramFiles(x86)", r"Mozilla Firefox\firefox.exe"),
                env_path("LOCALAPPDATA", r"Mozilla Firefox\firefox.exe"),
            ]),
            extensions_page: "about:debugging#/runtime/this-firefox",
            store: STORE_FIREFOX,
            firefox: true,
            tint: rgb(0xe8, 0x6a, 0x10),
        },
        Browser {
            name: "Opera",
            exe: first_existing(vec![
                env_path("LOCALAPPDATA", r"Programs\Opera\opera.exe"),
                env_path("LOCALAPPDATA", r"Programs\Opera GX\opera.exe"),
                env_path("ProgramFiles", r"Opera\opera.exe"),
            ]),
            extensions_page: "opera://extensions",
            store: STORE_OPERA,
            firefox: false,
            tint: rgb(0xd6, 0x1f, 0x2b),
        },
        Browser {
            name: "Brave",
            exe: first_existing(vec![
                env_path("ProgramFiles", r"BraveSoftware\Brave-Browser\Application\brave.exe"),
                env_path("ProgramFiles(x86)", r"BraveSoftware\Brave-Browser\Application\brave.exe"),
                env_path("LOCALAPPDATA", r"BraveSoftware\Brave-Browser\Application\brave.exe"),
            ]),
            extensions_page: "brave://extensions/",
            store: STORE_BRAVE,
            firefox: false,
            tint: rgb(0xfb, 0x54, 0x2b),
        },
    ]
}

/// The extension as it came with this program: when a file of its folder is
/// missing or was changed (an antivirus, a half-finished copy...), the
/// browser says "files missing or unreadable" and won't load it, so the
/// folder is put back in order from this copy first.
const EXTENSION_FILES: &[(&str, &[u8])] = &[
    ("background.js", include_bytes!("../../../extension/background.js")),
    ("early.js", include_bytes!("../../../extension/early.js")),
    ("content.js", include_bytes!("../../../extension/content.js")),
    ("yt-core.js", include_bytes!("../../../extension/yt-core.js")),
    ("popup.html", include_bytes!("../../../extension/popup.html")),
    ("popup.js", include_bytes!("../../../extension/popup.js")),
    ("options.html", include_bytes!("../../../extension/options.html")),
    ("options.js", include_bytes!("../../../extension/options.js")),
    ("icons/icon16.png", include_bytes!("../../../extension/icons/icon16.png")),
    ("icons/icon32.png", include_bytes!("../../../extension/icons/icon32.png")),
    ("icons/icon48.png", include_bytes!("../../../extension/icons/icon48.png")),
    ("icons/icon128.png", include_bytes!("../../../extension/icons/icon128.png")),
];
const MANIFEST: &[u8] = include_bytes!("../../../extension/manifest.json");
const MANIFEST_FIREFOX: &[u8] = include_bytes!("../../../extension/manifest.firefox.json");

/// Every file of the extension in `dir`, as it came with the program (only
/// what differs is written). false = it could not be written there.
pub(crate) fn put_extension(dir: &Path, firefox: bool) -> bool {
    let manifest = if firefox { MANIFEST_FIREFOX } else { MANIFEST };
    // the program's secret key: an add-in loaded from this folder reads it
    // and shows it with every request (other programs don't get in)
    let key = format!(
        "{{\"token\":\"{}\",\"port\":{}}}\n",
        crate::api::TOKEN.get().cloned().unwrap_or_default(),
        crate::api::MAIN_PORT.load(std::sync::atomic::Ordering::Relaxed)
    );
    let mut all: Vec<(&str, &[u8])> = EXTENSION_FILES.to_vec();
    all.push(("manifest.json", manifest));
    if crate::api::TOKEN.get().is_some() {
        all.push(("token.json", key.as_bytes()));
    }
    let mut ok = true;
    for (name, data) in all {
        let path = dir.join(name);
        if std::fs::read(&path).map(|have| have == data).unwrap_or(false) {
            continue;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // through a new file, so the browser never reads a half one
        let tmp = path.with_extension("ezoo-new");
        if std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
            let _ = std::fs::remove_file(&tmp);
            ok = false;
        }
    }
    ok
}

/// At start: the add-in folders next to the program are complete and carry
/// the key (an add-in loaded from them before gets the key at once).
pub(crate) fn prepare_extension_folders() {
    // right now (not later): an add-in that reloads after an update reads
    // the key at once
    for firefox in [false, true] {
        let _ = extension_dir(firefox);
    }
}

/// "Repair the browser add-in": every file of the add-in folders is written
/// again as it came with the program, and the add-in (it sees the new
/// number) forgets what it kept and starts again.
pub(crate) fn repair_extension() -> bool {
    let mut ok = true;
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        for (name, firefox) in [("extension", false), ("extension-firefox", true)] {
            let d = dir.join(name);
            if d.is_dir() {
                // what is not part of the add-in any more goes too
                if let Ok(rd) = std::fs::read_dir(&d) {
                    for e in rd.flatten() {
                        let n = e.file_name().to_string_lossy().to_string();
                        let known = n == "icons" || n == "manifest.json" || n == "token.json" || EXTENSION_FILES.iter().any(|(f, _)| *f == n);
                        if !known {
                            let p = e.path();
                            let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                        }
                    }
                }
                ok &= put_extension(&d, firefox);
            }
        }
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1);
    crate::api::REPAIR.store(now, std::sync::atomic::Ordering::Relaxed);
    ok
}

/// The extension's folder next to the program (Firefox has its own copy),
/// complete. Where the program's folder can't be written, a copy in the
/// program's data folder is used.
fn extension_dir(firefox: bool) -> Option<PathBuf> {
    let name = if firefox { "extension-firefox" } else { "extension" };
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let next_to = dir.join(name);
    if put_extension(&next_to, firefox) {
        return Some(next_to);
    }
    let own = swiftget_engine::default_data_dir().join(name);
    put_extension(&own, firefox).then_some(own)
}

/// The Chromium add-in folders of this program (next to it, and the copy in
/// its data folder where it could not write there): an add-in loaded from
/// one of them gets its id from that folder.
pub(crate) fn extension_folders() -> Vec<PathBuf> {
    let mut all = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        all.push(dir.join("extension"));
    }
    all.push(swiftget_engine::default_data_dir().join("extension"));
    all.into_iter().filter(|p| p.is_dir()).collect()
}

/// The id of our Firefox add-on (from its manifest).
pub(crate) fn firefox_addon_id() -> String {
    serde_json::from_slice::<serde_json::Value>(MANIFEST_FIREFOX)
        .ok()
        .and_then(|v| v["browser_specific_settings"]["gecko"]["id"].as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The signed Firefox add-on next to the program, when the release has one.
fn firefox_xpi() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    Some(dir.join(FIREFOX_XPI)).filter(|p| p.exists()).or_else(|| Some(fetched_xpi()).filter(|p| p.exists()))
}

/// The signed Firefox add-on of this very version, fetched from the
/// release when the setup had none (a build made before Mozilla signed it).
fn fetched_xpi() -> PathBuf {
    swiftget_engine::default_data_dir().join(format!("ezoo-download-firefox-{}.xpi", env!("CARGO_PKG_VERSION")))
}

/// Gets the signed add-on of this version from the official release page.
async fn fetch_firefox_xpi(proxy: swiftget_engine::http::ProxyConfig) -> anyhow::Result<PathBuf> {
    let url = format!(
        "https://github.com/AhmedEzo686/ezoo-download/releases/download/v{}/{FIREFOX_XPI}",
        env!("CARGO_PKG_VERSION")
    );
    let bytes = swiftget_engine::http::fetch_small(&proxy, &url).await?;
    // a zip (an add-on), not a web page
    anyhow::ensure!(bytes.starts_with(b"PK"), "not an add-on");
    let path = fetched_xpi();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = path.with_extension("xpi.part");
    std::fs::write(&part, &bytes)?;
    std::fs::rename(&part, &path)?;
    Ok(path)
}

/// Firefox opens the signed add-on ("Add Ezoo Download?"); fetched first
/// when this program has none. `done` gets what to show (`Err`: why not).
fn open_signed_in_firefox(ui: &Ui, exe: PathBuf, done: impl FnOnce(Result<PathBuf, String>) + Send + 'static) {
    if let Some(xpi) = firefox_xpi() {
        let _ = std::process::Command::new(&exe).arg(&xpi).spawn();
        return done(Ok(xpi));
    }
    let proxy = swiftget_engine::http::ProxyConfig::from_settings(&ui.ctx.mgr.settings());
    ui.ctx.rt.spawn(async move {
        let r = fetch_firefox_xpi(proxy).await;
        if let Ok(xpi) = &r {
            let _ = std::process::Command::new(&exe).arg(xpi).spawn();
        }
        done(r.map_err(|e| format!("{e:#}")));
    });
}

/// "Update the Firefox add-on": Firefox opens the signed add-on of this
/// program and asks "Add Ezoo Download?" (the new one takes the old one's
/// place). Without the file (a build without it): the browsers window.
pub(crate) fn update_firefox_addon(ui: &Ui) {
    let Some(exe) = detect().into_iter().find(|b| b.firefox).and_then(|b| b.exe) else { return open(ui) };
    open_signed_in_firefox(ui, exe, move |r| {
        if r.is_err() {
            let _ = slint::invoke_from_event_loop(|| HOME.with(|h| {
                if let Some(ui) = h.borrow().as_ref() {
                    open(ui);
                }
            }));
        }
    });
}

pub(crate) fn open(ui: &Ui) {
    let Some(d) = ui.browsers.upgrade() else { return };
    let list = detect();
    let rows: Vec<BrowserRow> = list
        .iter()
        .map(|b| BrowserRow {
            name: b.name.into(),
            tint: b.tint,
            installed: b.exe.is_some(),
            action: t(if b.store.is_empty() { "Click to add" } else { "Click to add from the store" }).into(),
        })
        .collect();
    *ui.browser_list.borrow_mut() = list;
    d.set_browsers(ModelRc::new(VecModel::from(rows)));
    d.set_steps(SharedString::new());
    d.set_folder(extension_dir(false).map(|p| p.display().to_string()).unwrap_or_default().into());
    open_window!(d);
    d.window().set_minimized(false);
}

fn launch(exe: &Path, url: &str) {
    let _ = std::process::Command::new(exe).arg(url).spawn();
}

fn add(ui: &Ui, i: usize) {
    let Some(d) = ui.browsers.upgrade() else { return };
    let Some(b) = ui.browser_list.borrow().get(i).cloned() else { return };
    let Some(exe) = b.exe.clone() else {
        d.set_steps(tf("{} is not installed on this computer.", &[&b.name]).into());
        return;
    };
    if !b.store.is_empty() {
        launch(&exe, b.store);
        d.set_steps(tf("{} opened the store page: click \"Add\" / \"Get\" there.", &[&b.name]).into());
        return;
    }
    if b.firefox {
        // the signed add-on (fetched from the official page when this
        // program has none): Firefox asks "Add Ezoo Download?" and keeps it.
        // Never the temporary add-on, which Firefox drops when it closes
        if firefox_xpi().is_none() {
            d.set_steps(t("Getting the signed Firefox add-on from the official page…").into());
        }
        let w = d.as_weak();
        open_signed_in_firefox(ui, exe, move |r| {
            let _ = slint::invoke_from_event_loop(move || {
                let Some(d) = w.upgrade() else { return };
                match r {
                    Ok(xpi) => {
                        d.set_folder(xpi.display().to_string().into());
                        d.set_steps(t("Firefox opened the Ezoo Download add-on.\n1. Click \"Add\" in the message Firefox shows.\n2. Then click \"OK\".\n\nThe add-on is signed by Mozilla, so it stays in Firefox after you close it and updates by itself.").into());
                    }
                    Err(e) => d.set_steps(
                        tf("The Firefox add-on could not be fetched ({}). Check the internet and click Firefox again.", &[&crate::err_text(&e)]).into(),
                    ),
                }
            });
        });
        return;
    }
    let Some(dir) = extension_dir(b.firefox) else {
        d.set_steps(t("The extension folder was not found next to EzooDownload.exe. Install Ezoo Download again.").into());
        return;
    };
    let path = dir.display().to_string();
    if let Ok(mut c) = arboard::Clipboard::new() {
        let _ = c.set_text(path.clone());
    }
    launch(&exe, b.extensions_page);
    d.set_folder(path.clone().into());
    let steps = if b.firefox {
        t("Firefox opened its add-ons debugging page.\n1. Click \"Load Temporary Add-on…\".\n2. Paste the folder path (Ctrl+V, it is already copied) and open the file \"manifest.json\" in it.\n\nFirefox removes add-ons loaded this way when it closes. To keep it for good the extension must be signed by Mozilla (free) – ask for the signed version.")
    } else {
        tf("{} opened its extensions page.\n1. Turn on \"Developer mode\" (a switch on that page).\n2. Click \"Load unpacked\".\n3. Paste the folder path (Ctrl+V – it is already copied) and click \"Select folder\".\n\nThat's all: the Ezoo Download icon appears next to the address bar. You do this only once.", &[&b.name])
    };
    d.set_steps(steps.into());
}

thread_local! {
    /// the program, for work that ends on another thread
    static HOME: std::cell::RefCell<Option<Ui>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn wire(ui: &Ui) {
    HOME.with(|h| *h.borrow_mut() = Some(ui.clone()));
    let Some(d) = ui.browsers.upgrade() else { return };
    {
        let ui = ui.clone();
        d.on_add(move |i| add(&ui, i.max(0) as usize));
    }
    {
        let w = d.as_weak();
        d.on_copy_folder(move || {
            if let Some(d) = w.upgrade() {
                if let Ok(mut c) = arboard::Clipboard::new() {
                    let _ = c.set_text(d.get_folder().to_string());
                }
            }
        });
    }
    {
        let w = d.as_weak();
        d.on_open_folder(move || {
            if let Some(d) = w.upgrade() {
                let f = d.get_folder().to_string();
                if !f.is_empty() {
                    let _ = open::that_detached(f);
                }
            }
        });
    }
    d.on_test_link(|| crate::message(&link_report()));
    {
        d.on_repair(move || {
            let ok = repair_extension();
            crate::message(&if ok {
                t("The add-in's files were put back. It starts again by itself within a minute (or click Reload on it in the browser's extensions page).")
            } else {
                t("Some files of the add-in could not be written. Close the browser and click \"Repair the add-in\" again.")
            });
        });
    }
    {
        let w = d.as_weak();
        d.on_close_window(move || {
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
        });
    }
}

/// "Test the link": what works between the browsers and the program, and
/// what to do about what does not.
pub(crate) fn link_report() -> String {
    use std::sync::atomic::Ordering;
    let now = crate::api::now_secs();
    let ago = |at: u64| at > 0 && now.saturating_sub(at) < 15 * 60;
    let mark = |ok: bool| if ok { "✔" } else { "✖" };
    let mut lines = vec![t("Link between the browsers and Ezoo Download:"), String::new()];
    let main = crate::api::MAIN_PORT.load(Ordering::Relaxed);
    lines.push(format!("{} {}", mark(main != 0), t("The program answers the add-in")));
    if !crate::api::FRONT_OPEN.load(Ordering::Relaxed) {
        lines.push(format!("• {}", t("Its usual port is kept by another program or by Windows: the add-in uses the program's own port (nothing to do).")));
    }
    let (helper, browsers) = crate::nativehost::report();
    lines.push(format!("{} {}", mark(helper), t("The browsers' helper is in place")));
    for (name, ok) in &browsers {
        lines.push(format!("   {} {}", mark(*ok), tf("{} knows the helper", &[name])));
    }
    let seen = crate::api::ADDIN_SEEN.load(Ordering::Relaxed);
    let helped = crate::api::HELPER_SEEN.load(Ordering::Relaxed);
    lines.push(format!("{} {}", mark(ago(seen)), t("An add-in talked to the program in the last 15 minutes")));
    lines.push(format!("{} {}", mark(ago(helped)), t("...through the browsers' helper")));
    lines.push(String::new());
    if !ago(seen) {
        lines.push(t("No add-in talked lately: open the browser, make sure the Ezoo Download add-in is on (Extensions page), then click a download link and test again."));
    } else if !ago(helped) {
        lines.push(t("The add-in talks through the port only. In Firefox: the add-in's window > \"Connect directly\". In other browsers: reload the add-in once (Extensions page)."));
    } else {
        lines.push(t("Everything works."));
    }
    lines.join("\n")
}
