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

#[cfg(windows)]
fn env_path(var: &str, rest: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|v| PathBuf::from(v).join(rest))
}

/// A program on PATH (Linux: chrome, firefox... are plain names).
fn on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).map(|d| d.join(name)).find(|p| p.is_file())
}

fn first_existing(list: Vec<Option<PathBuf>>) -> Option<PathBuf> {
    list.into_iter().flatten().find(|p| p.exists())
}

#[cfg(windows)]
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

#[cfg(not(windows))]
fn detect() -> Vec<Browser> {
    let rgb = slint::Color::from_rgb_u8;
    vec![
        Browser {
            name: "Chrome",
            exe: first_existing(vec![
                on_path("google-chrome"),
                on_path("google-chrome-stable"),
                on_path("chromium"),
                on_path("chromium-browser"),
            ]),
            extensions_page: "chrome://extensions/",
            store: STORE_CHROME,
            firefox: false,
            tint: rgb(0x1f, 0xa4, 0x63),
        },
        Browser {
            name: "Edge",
            exe: first_existing(vec![on_path("microsoft-edge"), on_path("microsoft-edge-stable")]),
            extensions_page: "edge://extensions/",
            store: STORE_EDGE,
            firefox: false,
            tint: rgb(0x0c, 0x8c, 0xd8),
        },
        Browser {
            name: "Firefox",
            exe: first_existing(vec![
                on_path("firefox"),
                on_path("firefox-esr"),
                Some(PathBuf::from("/usr/lib/firefox/firefox")),
            ]),
            extensions_page: "about:debugging#/runtime/this-firefox",
            store: STORE_FIREFOX,
            firefox: true,
            tint: rgb(0xe8, 0x6a, 0x10),
        },
        Browser {
            name: "Opera",
            exe: first_existing(vec![on_path("opera"), Some(PathBuf::from("/usr/bin/opera"))]),
            extensions_page: "opera://extensions",
            store: STORE_OPERA,
            firefox: false,
            tint: rgb(0xd6, 0x1f, 0x2b),
        },
        Browser {
            name: "Brave",
            exe: first_existing(vec![
                on_path("brave-browser"),
                on_path("brave"),
                Some(PathBuf::from("/usr/bin/brave-browser")),
            ]),
            extensions_page: "brave://extensions/",
            store: STORE_BRAVE,
            firefox: false,
            tint: rgb(0xfb, 0x54, 0x2b),
        },
    ]
}

/// The extension's folder next to the program (Firefox has its own copy).
/// On Linux the .deb also puts a copy under /usr/share/ezoo-download.
fn extension_dir(firefox: bool) -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let name = if firefox { "extension-firefox" } else { "extension" };
    [
        dir.join(name),
        dir.join("..").join(name),
        dir.join("..").join("share").join("ezoo-download").join(name),
        PathBuf::from("/usr/share/ezoo-download").join(name),
    ]
    .into_iter()
    .find(|p| p.exists())
}

/// The signed Firefox add-on next to the program, when the release has one.
fn firefox_xpi() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    [
        dir.join(FIREFOX_XPI),
        dir.join("..").join(FIREFOX_XPI),
        dir.join("..").join("share").join("ezoo-download").join(FIREFOX_XPI),
        PathBuf::from("/usr/share/ezoo-download").join(FIREFOX_XPI),
    ]
    .into_iter()
    .find(|p| p.exists())
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
                    let _ = open::that(f);
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
