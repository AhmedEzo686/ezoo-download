//! Guided extension setup, stable managed files, and observed browser readiness.

use crate::open_window;
use crate::*;
use crate::i18n::t;

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
    signed_firefox_in(&std::env::current_exe().ok()?, &api::bundled_extension_version())
}

fn signed_firefox_in(exe: &Path, expected: &str) -> Option<PathBuf> {
    // An upgrade may leave a stale signed XPI behind. Only offer the one
    // explicitly packaged for this bundled extension version.
    let version = resource_in(exe, "ezoo-download-firefox.version")?;
    if std::fs::read_to_string(version).ok()?.trim() != expected { return None; }
    resource_in(exe, FIREFOX_XPI).filter(|path| path.is_file())
}

fn text(en: &str, ar: &str) -> String {
    if crate::i18n::is_arabic() { ar.into() } else { t(en) }
}

fn selected(ui: &Ui) -> Option<Browser> {
    let d = ui.browsers.upgrade()?;
    ui.browser_list.borrow().get(d.get_current_browser().max(0) as usize).cloned()
}

thread_local! {
    static CONNECTION_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    static SETUP_ERROR: Cell<bool> = const { Cell::new(false) };
}

fn stop_connection_timer() {
    CONNECTION_TIMER.with(|timer| {
        if let Some(timer) = timer.borrow().as_ref() { timer.stop(); }
    });
}

pub(crate) fn open(ui: &Ui) {
    let Some(d) = ui.browsers.upgrade() else { return };
    let list: Vec<_> = detect().into_iter().filter(|b| b.exe.is_some()).collect();
    d.set_has_browsers(!list.is_empty());
    d.set_browser_names(ModelRc::new(VecModel::from(list.iter().map(|b| SharedString::from(b.name)).collect::<Vec<_>>())));
    *ui.browser_list.borrow_mut() = list;
    d.set_current_browser(0);
    configure(ui);
    open_window!(d);
    d.window().set_minimized(false);
    stop_connection_timer();
    let timer = slint::Timer::default();
    let ui2 = ui.clone();
    timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
        if ui2.browsers.upgrade().is_some_and(|d| d.window().is_visible()) {
            refresh_connection(&ui2);
        } else {
            stop_connection_timer();
        }
    });
    CONNECTION_TIMER.with(|slot| *slot.borrow_mut() = Some(timer));
}

fn configure(ui: &Ui) {
    SETUP_ERROR.with(|error| error.set(false));
    let Some(d) = ui.browsers.upgrade() else { return };
    d.set_setup_title(text("Connect Your Browser", "اربط متصفحك بالبرنامج").into());
    d.set_setup_detail(text("Send downloads and videos straight to Ezoo Download. Choose your browser, then start setup. If the browser asks, choose your usual profile.", "أرسل التنزيلات والفيديوهات مباشرة إلى Ezoo Download. اختر متصفحك ثم ابدأ الإعداد. إذا طلب المتصفح ذلك، اختر ملفك الشخصي المعتاد.").into());
    d.set_connected(false);
    d.set_notice(SharedString::new());
    d.set_folder(SharedString::new());
    d.set_prepared(false);
    d.set_setup_action(text("Start Setup…", "ابدأ الإعداد…").into());
    let Some(b) = selected(ui) else {
        d.set_connection_title(text("No Supported Browser Found", "لم يُعثر على متصفح مدعوم").into());
        d.set_connection_detail(text("Install Chrome, Edge, Firefox, Opera or Brave, then reopen this window. Safari is not supported.", "ثبّت Chrome أو Edge أو Firefox أو Opera أو Brave ثم أعد فتح هذه النافذة. Safari غير مدعوم.").into());
        d.set_fallback(d.get_connection_detail());
        return;
    };
    if let Some(dir) = crate::browser_files::prepared_dir(&default_data_dir(), b.firefox) {
        d.set_folder(dir.display().to_string().into());
        d.set_prepared(true);
    }
    if !b.store.is_empty() || (b.firefox && firefox_xpi().is_some()) {
        d.set_first_title(text("Add the Extension", "أضف الإضافة").into());
        d.set_first_detail(text("Approve adding Ezoo Download in the browser window.", "وافق على إضافة Ezoo Download في نافذة المتصفح.").into());
        d.set_second_title(text("Return Here", "ارجع إلى هنا").into());
        d.set_second_detail(text("This window will confirm when the extension connects.", "ستظهر رسالة تأكيد هنا عند اتصال الإضافة.").into());
        d.set_fallback(text("Open the extension in your browser and keep Ezoo Download running.", "افتح الإضافة في المتصفح واترك Ezoo Download قيد التشغيل.").into());
    } else if b.firefox {
        d.set_first_title(text("Load a Temporary Add-on", "حمّل إضافة مؤقتة").into());
        d.set_first_detail(text("On Firefox's debugging page, click Load Temporary Add-on…", "في صفحة تصحيح Firefox، اضغط «تحميل إضافة مؤقتة…».").into());
        d.set_second_title(text("Choose manifest.json", "اختر manifest.json").into());
        d.set_second_detail(text("Choose manifest.json inside the extension folder shown in Finder or Explorer.", "اختر manifest.json داخل مجلد الإضافة الظاهر في Finder أو مستكشف الملفات.").into());
        d.set_notice(text("Firefox removes temporary add-ons when it closes. Permanent installation needs a Mozilla-signed release.", "يحذف Firefox الإضافات المؤقتة عند إغلاقه. التثبيت الدائم يحتاج إصدارًا موقّعًا من Mozilla.").into());
        d.set_fallback(file_picker_help(true).into());
    } else {
        d.set_first_title(text("Turn On Developer Mode", "فعّل وضع المطوّر").into());
        d.set_first_detail(text("Use the Developer mode switch on the browser's extensions page.", "فعّل مفتاح «وضع المطوّر» في صفحة إضافات المتصفح.").into());
        if b.name == "Chrome" {
            d.set_second_title(text("Drag the Extension Folder", "اسحب مجلد الإضافة").into());
            d.set_second_detail(text("Drag the selected Ezoo Download Extension folder from Finder or Explorer onto the Chrome extensions page. Chrome will add it.", "اسحب مجلد Ezoo Download Extension المحدد من Finder أو مستكشف الملفات إلى صفحة إضافات Chrome لإضافته.").into());
        } else {
            d.set_second_title(text("Load the Extension Folder", "حمّل مجلد الإضافة").into());
            d.set_second_detail(text("Click Load unpacked and choose the prepared Ezoo Download Extension folder. Open Having Trouble? for the file-picker shortcut.", "اضغط «تحميل إضافة غير مضغوطة» واختر مجلد Ezoo Download Extension المجهز. افتح «هل تواجه مشكلة؟» لمعرفة اختصار اختيار المجلد.").into());
        }
        d.set_notice(text("Keep the prepared folder in its current location. Ezoo Download will update it for you.", "اترك المجلد المجهز في مكانه الحالي؛ سيحدّثه Ezoo Download تلقائيًا.").into());
        d.set_fallback(file_picker_help(false).into());
    }
    refresh_connection(ui);
}

fn file_picker_help(firefox: bool) -> String {
    let finish = if firefox { text("Select manifest.json and click Open.", "اختر manifest.json واضغط «فتح».") } else { text("Click Open / Select Folder to use this folder.", "اضغط «فتح / اختيار المجلد» لاستخدام هذا المجلد.") };
    if cfg!(target_os = "macos") {
        format!("{}\n\n{}\n{}", text("Use the file picker instead:", "استخدم نافذة اختيار الملفات بدلًا من السحب:"), text("Click Copy Folder Path below. In the browser's file picker, press Command–Shift–G, paste with Command–V, then press Return.", "اضغط «نسخ مسار المجلد» أدناه. في نافذة اختيار الملفات بالمتصفح، اضغط Command–Shift–G ثم الصق باستخدام Command–V واضغط Return."), finish)
    } else {
        format!("{}\n{}", text("Click Copy Folder Path below. In the browser's file picker, paste it in the address bar with Ctrl+V and press Enter.", "اضغط «نسخ مسار المجلد» أدناه. في نافذة اختيار الملفات بالمتصفح، الصق المسار في شريط العنوان باستخدام Ctrl+V واضغط Enter."), finish)
    }
}

fn refresh_connection(ui: &Ui) {
    let Some(d) = ui.browsers.upgrade() else { return };
    let Some(b) = selected(ui) else { return };
    let status = api::extension_status(b.name);
    let current = status.as_ref().is_some_and(|s| s.version == api::bundled_extension_version());
    if SETUP_ERROR.with(Cell::get) { return; }
    d.set_connected(current);
    if current {
        SETUP_ERROR.with(|error| error.set(false));
        d.set_prepared(true);
        d.set_setup_action(text("Open Browser…", "افتح المتصفح…").into());
        let enabled = status.is_some_and(|s| s.enabled);
        d.set_connection_title(text(if enabled { "Connected" } else { "Connected · Downloads Paused" }, if enabled { "متصل" } else { "متصل · التنزيلات متوقفة مؤقتًا" }).into());
        d.set_connection_detail(text(if enabled { "The extension is communicating with Ezoo Download. Setup is complete." } else { "Turn the extension on from its browser popup when you want it to handle downloads." }, if enabled { "الإضافة تتواصل مع Ezoo Download. اكتمل الإعداد." } else { "شغّل الإضافة من نافذتها في المتصفح عندما تريد استخدامها للتنزيلات." }).into());
    } else if status.is_some() {
        d.set_connection_title(text("Extension Update Needed", "الإضافة تحتاج إلى تحديث").into());
        d.set_connection_detail(text("Reopen the extensions page and click Reload for Ezoo Download, then open its popup once.", "أعد فتح صفحة الإضافات واضغط «إعادة تحميل» لإضافة Ezoo Download ثم افتح نافذتها مرة واحدة.").into());
    } else {
        d.set_setup_action(text(if d.get_prepared() { "Open Setup Again…" } else { "Start Setup…" }, if d.get_prepared() { "افتح الإعداد مجددًا…" } else { "ابدأ الإعداد…" }).into());
        d.set_connection_title(text(if d.get_prepared() { "Waiting for Your Browser" } else { "Not Connected Yet" }, if d.get_prepared() { "في انتظار المتصفح" } else { "لم يتم الاتصال بعد" }).into());
        d.set_connection_detail(text(if d.get_prepared() { "Complete the browser steps above. If it is already installed, open the Ezoo Download extension popup once." } else { "Start setup to prepare the extension and open the right browser page." }, if d.get_prepared() { "أكمل الخطوات أعلاه في المتصفح. إذا كانت الإضافة مثبتة، افتح نافذة إضافة Ezoo Download مرة واحدة." } else { "ابدأ الإعداد لتجهيز الإضافة وفتح الصفحة المناسبة في المتصفح." }).into());
    }
}

fn launch(exe: &Path, url: &str) -> anyhow::Result<()> {
    launch_argument(exe, std::ffi::OsStr::new(url))
}

fn launch_argument(exe: &Path, argument: &std::ffi::OsStr) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        // Use the normal macOS URL/document handoff to the running browser.
        // Starting Chrome's executable directly can reopen its profile picker
        // and lose the requested page, even with an existing profile window.
        let application = exe.ancestors().find(|path| path.extension().is_some_and(|ext| ext == "app"))
            .ok_or_else(|| anyhow::anyhow!("The browser app bundle was not found."))?;
        let status = std::process::Command::new("/usr/bin/open").arg("-a").arg(application).arg(argument).status()?;
        anyhow::ensure!(status.success(), "The browser could not open the setup page.");
    }
    #[cfg(not(target_os = "macos"))]
    { std::process::Command::new(exe).arg(argument).spawn()?; }
    Ok(())
}

fn reveal(path: &Path) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    { std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn()?; }
    #[cfg(windows)]
    { std::process::Command::new("explorer.exe").arg(format!("/select,{}", path.display())).spawn()?; }
    #[cfg(not(any(windows, target_os = "macos")))]
    { open::that(path.parent().unwrap_or(path))?; }
    Ok(())
}

fn show_error(ui: &Ui, error: anyhow::Error) {
    SETUP_ERROR.with(|error| error.set(true));
    if let Some(d) = ui.browsers.upgrade() {
        d.set_connection_title(text("Setup Could Not Finish", "تعذّر إكمال الإعداد").into());
        d.set_connection_detail(format!("{} {error}", text("Try again, or reinstall the latest Ezoo Download release.", "حاول مجددًا أو أعد تثبيت أحدث إصدار من Ezoo Download.")).into());
    }
}

fn start_setup(ui: &Ui) -> anyhow::Result<()> {
    SETUP_ERROR.with(|error| error.set(false));
    let Some(d) = ui.browsers.upgrade() else { return Ok(()) };
    let Some(b) = selected(ui) else { return Ok(()) };
    let Some(exe) = b.exe.as_deref() else { return Ok(()) };
    if d.get_connected() { return launch(exe, b.extensions_page); }
    if !b.store.is_empty() {
        launch(exe, b.store)?;
        d.set_prepared(true);
    } else if let Some(xpi) = b.firefox.then(firefox_xpi).flatten() {
        launch_argument(exe, xpi.as_os_str())?;
        d.set_prepared(true);
    } else {
        let source = extension_dir(b.firefox).ok_or_else(|| anyhow::anyhow!("The bundled extension files are missing."))?;
        let dir = crate::browser_files::prepare(&source, &default_data_dir(), b.firefox, ui.ctx.mgr.settings().browser_port)?;
        d.set_folder(dir.display().to_string().into());
        d.set_prepared(true);
        // Reveal only the managed folder; never open or edit browser profiles.
        reveal(&dir)?;
        launch(exe, b.extensions_page)?;
    }
    refresh_connection(ui);
    Ok(())
}

/// Refresh only previously prepared, app-managed folders at startup. Nothing
/// is installed into the browser and no browser profile is accessed here.
pub(crate) fn refresh_prepared_extensions(port: u16) {
    let data = default_data_dir();
    for firefox in [false, true] {
        if crate::browser_files::prepared_dir(&data, firefox).is_some() {
            if let Some(source) = extension_dir(firefox) {
                if let Err(error) = crate::browser_files::prepare(&source, &data, firefox, port) {
                    eprintln!("Could not refresh browser extension: {error}");
                }
            }
        }
    }
}

pub(crate) fn wire(ui: &Ui) {
    let Some(d) = ui.browsers.upgrade() else { return };
    {
        let ui = ui.clone();
        d.on_select_browser(move |_| configure(&ui));
    }
    {
        let ui = ui.clone();
        d.on_start_setup(move || { if let Err(error) = start_setup(&ui) { show_error(&ui, error); } });
    }
    {
        let ui = ui.clone();
        d.on_reopen_browser(move || {
            if let Some(b) = selected(&ui) {
                if let Some(exe) = b.exe.as_deref() {
                    if let Err(error) = launch(exe, if b.store.is_empty() { b.extensions_page } else { b.store }) { show_error(&ui, error); }
                }
            }
        });
    }
    {
        let w = d.as_weak();
        d.on_copy_folder(move || {
            if let Some(d) = w.upgrade() {
                if let Ok(mut c) = arboard::Clipboard::new() { let _ = c.set_text(d.get_folder().to_string()); }
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_open_folder(move || {
            if let Some(d) = ui.browsers.upgrade() {
                if !d.get_folder().is_empty() {
                    if let Err(error) = reveal(Path::new(d.get_folder().as_str())) { show_error(&ui, error); }
                }
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_check_connection(move || {
            SETUP_ERROR.with(|error| error.set(false));
            refresh_connection(&ui);
        });
    }
    {
        let w = d.as_weak();
        d.on_close_window(move || {
            stop_connection_timer();
            if let Some(d) = w.upgrade() { let _ = d.hide(); }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{resource_in, signed_firefox_in};

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
        assert!(signed_firefox_in(&executable, "1.0.6.1").is_none());
        std::fs::write(resources.join("ezoo-download-firefox.version"), "1.0.4\n").unwrap();
        assert!(signed_firefox_in(&executable, "1.0.6.1").is_none());
        std::fs::write(resources.join("ezoo-download-firefox.version"), "1.0.6.1\n").unwrap();
        assert_eq!(signed_firefox_in(&executable, "1.0.6.1").unwrap().canonicalize().unwrap(), resources.join("ezoo-download-firefox.xpi").canonicalize().unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}
