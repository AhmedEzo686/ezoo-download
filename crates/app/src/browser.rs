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

fn browser_exe(windows_paths: Vec<Option<PathBuf>>, app: &str, executable: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let _ = windows_paths;
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
        }
        roots.into_iter()
            .map(|root| root.join(app).join("Contents/MacOS").join(executable))
            .find(|path| path.is_file())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, executable);
        windows_paths.into_iter().flatten().find(|path| path.is_file())
    }
}

fn detect() -> Vec<Browser> {
    let rgb = slint::Color::from_rgb_u8;
    vec![
        Browser {
            name: "Chrome",
            exe: browser_exe(vec![
                env_path("ProgramFiles", r"Google\Chrome\Application\chrome.exe"),
                env_path("ProgramFiles(x86)", r"Google\Chrome\Application\chrome.exe"),
                env_path("LOCALAPPDATA", r"Google\Chrome\Application\chrome.exe"),
            ], "Google Chrome.app", "Google Chrome"),
            extensions_page: "chrome://extensions/",
            store: STORE_CHROME,
            firefox: false,
            tint: rgb(0x1f, 0xa4, 0x63),
        },
        Browser {
            name: "Edge",
            exe: browser_exe(vec![
                env_path("ProgramFiles(x86)", r"Microsoft\Edge\Application\msedge.exe"),
                env_path("ProgramFiles", r"Microsoft\Edge\Application\msedge.exe"),
            ], "Microsoft Edge.app", "Microsoft Edge"),
            extensions_page: "edge://extensions/",
            store: STORE_EDGE,
            firefox: false,
            tint: rgb(0x0c, 0x8c, 0xd8),
        },
        Browser {
            name: "Firefox",
            exe: browser_exe(vec![
                env_path("ProgramFiles", r"Mozilla Firefox\firefox.exe"),
                env_path("ProgramFiles(x86)", r"Mozilla Firefox\firefox.exe"),
                env_path("LOCALAPPDATA", r"Mozilla Firefox\firefox.exe"),
            ], "Firefox.app", "firefox"),
            extensions_page: "about:debugging#/runtime/this-firefox",
            store: STORE_FIREFOX,
            firefox: true,
            tint: rgb(0xe8, 0x6a, 0x10),
        },
        Browser {
            name: "Opera",
            exe: browser_exe(vec![
                env_path("LOCALAPPDATA", r"Programs\Opera\opera.exe"),
                env_path("LOCALAPPDATA", r"Programs\Opera GX\opera.exe"),
                env_path("ProgramFiles", r"Opera\opera.exe"),
            ], "Opera.app", "Opera"),
            extensions_page: "opera://extensions",
            store: STORE_OPERA,
            firefox: false,
            tint: rgb(0xd6, 0x1f, 0x2b),
        },
        Browser {
            name: "Brave",
            exe: browser_exe(vec![
                env_path("ProgramFiles", r"BraveSoftware\Brave-Browser\Application\brave.exe"),
                env_path("ProgramFiles(x86)", r"BraveSoftware\Brave-Browser\Application\brave.exe"),
                env_path("LOCALAPPDATA", r"BraveSoftware\Brave-Browser\Application\brave.exe"),
            ], "Brave Browser.app", "Brave Browser"),
            extensions_page: "brave://extensions/",
            store: STORE_BRAVE,
            firefox: false,
            tint: rgb(0xfb, 0x54, 0x2b),
        },
    ]
}

/// Resources are beside a portable executable or in a macOS app bundle.
fn resource_in(executable: &Path, name: &str) -> Option<PathBuf> {
    let dir = executable.parent()?;
    [dir.join(name), dir.join("../Resources").join(name), dir.join("..").join(name)]
        .into_iter()
        .find(|path| path.exists())
}

/// The extension's folder (Firefox has its own copy).
fn extension_dir(firefox: bool) -> Option<PathBuf> {
    let name = if firefox { "extension-firefox" } else { "extension" };
    resource_in(&std::env::current_exe().ok()?, name).filter(|path| path.is_dir())
}

/// The signed Firefox add-on, when the release includes one.
fn firefox_xpi() -> Option<PathBuf> {
    resource_in(&std::env::current_exe().ok()?, FIREFOX_XPI).filter(|path| path.is_file())
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
        let missing = if cfg!(target_os = "macos") {
            t("The extension folder was not found. Install Ezoo Download again.")
        } else {
            t("The extension folder was not found next to EzooDownload.exe. Install Ezoo Download again.")
        };
        d.set_steps(missing.into());
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
    let steps = if cfg!(target_os = "macos") { steps.replace("Ctrl+V", "Command+V") } else { steps };
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

#[cfg(test)]
mod tests {
    use super::resource_in;

    #[test]
    fn finds_extensions_in_app_bundle_resources() {
        let root = std::env::temp_dir().join(format!("ezoo-app-resources-{}", std::process::id()));
        let contents = root.join("Ezoo Download.app/Contents");
        let resources = contents.join("Resources");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        std::fs::create_dir_all(resources.join("extension")).unwrap();
        std::fs::write(resources.join("ezoo-download-firefox.xpi"), b"test").unwrap();
        let executable = contents.join("MacOS/EzooDownload");
        assert_eq!(resource_in(&executable, "extension").unwrap().canonicalize().unwrap(), resources.join("extension").canonicalize().unwrap());
        assert_eq!(resource_in(&executable, "ezoo-download-firefox.xpi").unwrap().canonicalize().unwrap(), resources.join("ezoo-download-firefox.xpi").canonicalize().unwrap());
        assert!(resource_in(&executable, "extension-firefox").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
