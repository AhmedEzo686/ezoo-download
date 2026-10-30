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
    let all = EXTENSION_FILES.iter().copied().chain(std::iter::once(("manifest.json", manifest)));
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

/// The signed Firefox add-on next to the program, when the release has one.
fn firefox_xpi() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    Some(dir.join(FIREFOX_XPI)).filter(|p| p.exists())
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
        if let Some(xpi) = firefox_xpi() {
            // the signed add-on: Firefox asks "Add Ezoo Download?" and keeps it
            let _ = std::process::Command::new(&exe).arg(&xpi).spawn();
            d.set_folder(xpi.display().to_string().into());
            d.set_steps(t("Firefox opened the Ezoo Download add-on.\n1. Click \"Add\" in the message Firefox shows.\n2. Then click \"OK\".\n\nThe add-on is signed by Mozilla, so it stays in Firefox after you close it and updates by itself.").into());
            return;
        }
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

pub(crate) fn wire(ui: &Ui) {
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
    {
        let w = d.as_weak();
        d.on_close_window(move || {
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
        });
    }
}
