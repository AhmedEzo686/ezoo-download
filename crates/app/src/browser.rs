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

#[cfg_attr(not(windows), allow(dead_code))]
fn first_existing(list: Vec<Option<PathBuf>>) -> Option<PathBuf> {
    list.into_iter().flatten().find(|p| p.exists())
}

/// A program looked up in PATH (Linux and the other Unix systems keep their
/// browsers there: /usr/bin/google-chrome, /usr/bin/firefox...).
#[cfg(unix)]
fn in_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    names.iter().find_map(|name| dirs.iter().map(|d| d.join(name)).find(|p| p.is_file()))
}

/// A browser a package manager put outside PATH: a Flatpak (system or user)
/// or a Snap. Their launcher scripts take the browser's own arguments.
#[cfg(unix)]
fn packaged(flatpak_id: &str, snap: &str, extra: &[&str]) -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/var/lib/flatpak/exports/bin").join(flatpak_id),
        PathBuf::from("/snap/bin").join(snap),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".local/share/flatpak/exports/bin").join(flatpak_id));
    }
    // places distributions use instead of a link in PATH
    candidates.extend(extra.iter().map(PathBuf::from));
    candidates.into_iter().find(|p| p.is_file())
}

/// The program of a browser: its usual Windows places, or its name in PATH,
/// or the Flatpak / Snap it was installed as.
fn exe_of(
    windows: Vec<Option<PathBuf>>,
    unix_names: &[&str],
    flatpak_id: &str,
    snap: &str,
    extra: &[&str],
) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let _ = (unix_names, flatpak_id, snap, extra);
        first_existing(windows)
    }
    #[cfg(not(windows))]
    {
        let _ = windows;
        in_path(unix_names).or_else(|| packaged(flatpak_id, snap, extra))
    }
}

fn detect() -> Vec<Browser> {
    let rgb = slint::Color::from_rgb_u8;
    let mut list = vec![
        Browser {
            name: "Chrome",
            exe: exe_of(
                vec![
                    env_path("ProgramFiles", r"Google\Chrome\Application\chrome.exe"),
                    env_path("ProgramFiles(x86)", r"Google\Chrome\Application\chrome.exe"),
                    env_path("LOCALAPPDATA", r"Google\Chrome\Application\chrome.exe"),
                ],
                &["google-chrome-stable", "google-chrome"],
                "com.google.Chrome",
                "google-chrome",
                &["/opt/google/chrome/chrome"],
            ),
            extensions_page: "chrome://extensions/",
            store: STORE_CHROME,
            firefox: false,
            tint: rgb(0x1f, 0xa4, 0x63),
        },
        Browser {
            name: "Edge",
            exe: exe_of(
                vec![
                    env_path("ProgramFiles(x86)", r"Microsoft\Edge\Application\msedge.exe"),
                    env_path("ProgramFiles", r"Microsoft\Edge\Application\msedge.exe"),
                ],
                &["microsoft-edge-stable", "microsoft-edge"],
                "com.microsoft.Edge",
                "microsoft-edge",
                &[],
            ),
            extensions_page: "edge://extensions/",
            store: STORE_EDGE,
            firefox: false,
            tint: rgb(0x0c, 0x8c, 0xd8),
        },
        Browser {
            name: "Firefox",
            exe: exe_of(
                vec![
                    env_path("ProgramFiles", r"Mozilla Firefox\firefox.exe"),
                    env_path("ProgramFiles(x86)", r"Mozilla Firefox\firefox.exe"),
                    env_path("LOCALAPPDATA", r"Mozilla Firefox\firefox.exe"),
                ],
                &["firefox", "firefox-esr"],
                "org.mozilla.firefox",
                "firefox",
                &[],
            ),
            extensions_page: "about:debugging#/runtime/this-firefox",
            store: STORE_FIREFOX,
            firefox: true,
            tint: rgb(0xe8, 0x6a, 0x10),
        },
        Browser {
            name: "Opera",
            exe: exe_of(
                vec![
                    env_path("LOCALAPPDATA", r"Programs\Opera\opera.exe"),
                    env_path("LOCALAPPDATA", r"Programs\Opera GX\opera.exe"),
                    env_path("ProgramFiles", r"Opera\opera.exe"),
                ],
                &["opera", "opera-stable"],
                "com.opera.Opera",
                "opera",
                &["/usr/lib/x86_64-linux-gnu/opera/opera", "/usr/lib/opera/opera"],
            ),
            extensions_page: "opera://extensions",
            store: STORE_OPERA,
            firefox: false,
            tint: rgb(0xd6, 0x1f, 0x2b),
        },
        Browser {
            name: "Brave",
            exe: exe_of(
                vec![
                    env_path("ProgramFiles", r"BraveSoftware\Brave-Browser\Application\brave.exe"),
                    env_path("ProgramFiles(x86)", r"BraveSoftware\Brave-Browser\Application\brave.exe"),
                    env_path("LOCALAPPDATA", r"BraveSoftware\Brave-Browser\Application\brave.exe"),
                ],
                &["brave-browser", "brave"],
                "com.brave.Browser",
                "brave",
                &[],
            ),
            extensions_page: "brave://extensions/",
            store: STORE_BRAVE,
            firefox: false,
            tint: rgb(0xfb, 0x54, 0x2b),
        },
    ];
    // Linux distributions ship Chromium (Chrome without Google's parts)
    #[cfg(all(unix, not(target_os = "macos")))]
    list.insert(
        1,
        Browser {
            name: "Chromium",
            exe: exe_of(
                Vec::new(),
                &["chromium", "chromium-browser"],
                "org.chromium.Chromium",
                "chromium",
                &["/usr/lib/chromium/chromium"],
            ),
            extensions_page: "chrome://extensions/",
            store: "",
            firefox: false,
            tint: rgb(0x62, 0x8c, 0xd6),
        },
    );
    list
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

/// The extension's folder (Firefox has its own copy), complete.
///
/// On Linux the data folder is used even when the program's own folder is
/// writable: Chrome, Opera and the others remember the folder the user picked
/// once in "Load unpacked", so it must not move when the program is updated
/// or installed another way (.deb, AppImage, portable). On Windows the folder
/// next to the program keeps the old behaviour.
fn extension_dir(firefox: bool) -> Option<PathBuf> {
    let name = if firefox { "extension-firefox" } else { "extension" };
    #[cfg(not(windows))]
    {
        let own = swiftget_engine::default_data_dir().join(name);
        if put_extension(&own, firefox) {
            return Some(own);
        }
    }
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let next_to = dir.join(name);
    if put_extension(&next_to, firefox) {
        return Some(next_to);
    }
    #[cfg(windows)]
    {
        let own = swiftget_engine::default_data_dir().join(name);
        return put_extension(&own, firefox).then_some(own);
    }
    #[allow(unreachable_code)]
    None
}

/// The same extension in a folder the browser's file window opens at.
fn in_downloads(dir: &Path, firefox: bool) -> PathBuf {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return dir.to_path_buf() };
    let base = if home.join("Downloads").is_dir() { home.join("Downloads") } else { home };
    let target = base.join(if firefox { "EzooDownload-extension-firefox" } else { "EzooDownload-extension" });
    if put_extension(&target, firefox) {
        target
    } else {
        dir.to_path_buf()
    }
}

/// The signed Firefox add-on next to the program, when the release has one
/// (a package puts it in /usr/share/ezoodownload instead).
fn firefox_xpi() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let mut candidates = vec![dir.join(FIREFOX_XPI)];
    for d in ["/usr/share/ezoodownload", "/usr/local/share/ezoodownload"] {
        candidates.push(Path::new(d).join(FIREFOX_XPI));
    }
    candidates.push(swiftget_engine::default_data_dir().join(FIREFOX_XPI));
    candidates.into_iter().find(|p| p.exists())
}

pub(crate) fn open(ui: &Ui) {
    let Some(d) = crate::ensure_browsers(ui) else { return };
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
    // A folder in Downloads is easy to pick in the browser's "Load unpacked"
    // window; a hidden one under ~/.local is not (a Flatpak browser may not
    // resolve it at all). The same files, one more copy.
    let dir = in_downloads(&dir, b.firefox);
    if !b.firefox {
        // the packed add-on too, for the browsers that take a .crx by dropping it
        if let Some(me) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
            let crx = me.join("ezoo-download-chrome.crx");
            if crx.is_file() {
                let _ = std::fs::copy(&crx, dir.join("ezoo-download-chrome.crx"));
            }
        }
    }
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
