// Ezoo Download — desktop app.
// No console window in release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

slint::include_modules!();

mod api;
mod autostart;
mod browser;
mod clipboard;
mod extra;
mod grab;
mod i18n;
mod options;
mod playlist;
mod preview;
mod torrent_ui;
mod scheduler;
mod sound;
mod tray;
mod update;
mod winfx;

use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel, Weak};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use swiftget_engine::config::MAIN_QUEUE;
use i18n::{t, tf};
use swiftget_engine::filename;
use swiftget_engine::{
    default_data_dir, fmt_duration, fmt_size, AfterDone, DownloadKind, DownloadView, Event, Manager, NewDownload,
    ProbeResult, RequestHeaders, Settings, Status,
};

pub(crate) const APP: &str = "Ezoo Download";

/// What the browser extension sends along with a video link.
#[derive(Default, Clone)]
pub(crate) struct VideoExtra {
    /// audio stream merged with the video after download
    audio_url: Option<String>,
    /// page + quality key, used to renew expired links
    source_page: Option<String>,
    source_key: Option<String>,
}

/// State of the "Download File Info" window (shared with background threads).
#[derive(Default)]
pub(crate) struct AddState {
    headers: RequestHeaders,
    extra: VideoExtra,
    /// the URL the window was opened with (video extras only apply to it)
    origin_url: String,
    probe: Option<ProbeResult>,
    probe_url: String,
    probe_seq: u64,
    /// last "Save As" we filled in ourselves (so we don't overwrite the user's)
    auto_path: String,
    /// category names in the window's list (General first)
    categories: Vec<String>,
    /// download already running in the background for this window; it is
    /// not in the list (and not "complete") until Start is pressed
    early_id: Option<u64>,
    /// links that came in while the window was busy with another one
    waiting: std::collections::VecDeque<PendingOpen>,
}

/// A "Download File Info" request waiting for the window.
pub(crate) struct PendingOpen {
    url: String,
    headers: RequestHeaders,
    name_hint: Option<String>,
    extra: VideoExtra,
}

/// Everything background threads need. All fields are Send.
#[derive(Clone)]
pub(crate) struct Ctx {
    mgr: Arc<Manager>,
    rt: tokio::runtime::Handle,
    main: Weak<MainWindow>,
    add: Weak<AddDialog>,
    add_state: Arc<Mutex<AddState>>,
}

/// A progress window and its connections list (updated in place, so it
/// doesn't flicker when refreshed 10 times a second).
type ProgressWin = (DetailsDialog, Rc<VecModel<SegView>>);

/// A download waiting for the answer of the "Duplicate download link" window.
pub(crate) struct PendingAdd {
    req: NewDownload,
    existing: u64,
}

/// Downloads selected in the list (Ctrl / Shift click, Ctrl+A).
#[derive(Default)]
pub(crate) struct Selection {
    ids: Vec<u64>,
    /// where a Shift click range starts
    anchor: Option<u64>,
}

/// Things that only live on the UI thread.
#[derive(Clone)]
pub(crate) struct Ui {
    ctx: Ctx,
    rows: Rc<VecModel<DownloadRow>>,
    queues: Rc<VecModel<QueueRow>>,
    cats: Rc<VecModel<CatRow>>,
    /// one progress window per download
    progress: Rc<RefCell<HashMap<u64, ProgressWin>>>,
    batch: Weak<BatchDialog>,
    settings_dlg: Weak<SettingsDialog>,
    complete: Weak<CompleteDialog>,
    complete_id: Rc<Cell<Option<u64>>>,
    columns: Weak<ColumnsDialog>,
    category: Weak<CategoryDialog>,
    /// category being edited (None = a new one)
    editing_category: Rc<RefCell<Option<String>>>,
    find: Weak<FindDialog>,
    props: Weak<PropertiesDialog>,
    props_id: Rc<Cell<Option<u64>>>,
    dup: Weak<DuplicateDialog>,
    pending: Rc<RefCell<Option<PendingAdd>>>,
    countdown: Weak<CountdownDialog>,
    /// (action, seconds left)
    power: Rc<RefCell<Option<(AfterDone, i32)>>>,
    sched: Weak<SchedulerDialog>,
    sched_state: Rc<RefCell<scheduler::SchedState>>,
    grabber: Weak<GrabberDialog>,
    grab_state: Rc<RefCell<grab::GrabUi>>,
    tips: Weak<TipDialog>,
    refresh_dlg: Weak<RefreshDialog>,
    /// download waiting for a new address from the browser
    refresh_id: Rc<Cell<Option<u64>>>,
    options_lists: Rc<RefCell<options::Lists>>,
    sel: Rc<RefCell<Selection>>,
    playlist: Weak<PlaylistDialog>,
    playlist_rows: Rc<VecModel<LinkRow>>,
    torrent: Weak<TorrentDialog>,
    torrent_rows: Rc<VecModel<LinkRow>>,
    browsers: Weak<BrowserDialog>,
    updater: Weak<UpdateDialog>,
    /// the download list's columns (order + widths) as the header shows them
    columns_model: Rc<VecModel<ColDef>>,
    browser_list: Rc<RefCell<Vec<browser::Browser>>>,
}

/// One line in a small log next to the downloads list (data\events.log):
/// when the program started, closed, updated itself, could not start...
/// what someone asking for help can send.
pub(crate) fn note(what: &str) {
    let dir = default_data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("events.log");
    if std::fs::metadata(&path).map(|m| m.len() > 128 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
    let line = format!("{} {} {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), env!("CARGO_PKG_VERSION"), what);
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// A crash leaves a note (what and where, no personal data) in crash.log in
/// the program's data folder, so it can be found and fixed.
fn install_crash_log() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let path = default_data_dir().join("crash.log");
        // keep the file small: start again when it gets big
        if std::fs::metadata(&path).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
            let _ = std::fs::remove_file(&path);
        }
        let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let line = format!(
            "{} version {} thread {:?}: {} at {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            env!("CARGO_PKG_VERSION"),
            std::thread::current().name().unwrap_or("?"),
            what,
            place
        );
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(line.as_bytes());
        }
        previous(info);
    }));
}

fn main() {
    install_crash_log();
    let data_dir = default_data_dir();
    let gpu = Settings::load(&data_dir.join("settings.json")).gpu_drawing;
    // The windows are drawn by the processor unless the user chose the
    // graphics card: drawing with it needs a working OpenGL driver (without
    // one the program closed by itself), takes hundreds of MB on some cards,
    // and the progress window answered the mouse in the wrong place on some
    // Windows 10 computers.
    if std::env::var_os("SLINT_BACKEND").is_none() && !gpu {
        std::env::set_var("SLINT_BACKEND", "winit-software");
    }
    if let Err(e) = run() {
        let text = format!("{e:#}");
        note(&format!("could not start: {text}"));
        if gpu {
            // the graphics card could not draw: the next start uses the processor
            let path = data_dir.join("settings.json");
            let mut s = Settings::load(&path);
            s.gpu_drawing = false;
            let _ = s.save(&path);
        }
        let _ = rfd::MessageDialog::new()
            .set_title("Ezoo Download")
            .set_level(rfd::MessageLevel::Error)
            .set_description(format!(
                "Ezoo Download could not start:\n\n{text}\n\nتعذّر تشغيل Ezoo Download. أعد تشغيل الجهاز ثم جرّب مرة أخرى."
            ))
            .show();
        std::process::exit(1);
    }
}

/// Opened "as administrator" (for the log: the program never needs it).
fn elevated() -> bool {
    #[cfg(windows)]
    {
        // SAFETY: no arguments; it only reads the token of this process
        unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
    }
    #[cfg(not(windows))]
    false
}

/// End a copy of this program that hangs (only ever one of ours: the id
/// came from its own answer on our port, and the name is checked too).
fn end_process(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let exe = std::env::current_exe().ok().and_then(|e| e.file_name().map(|n| n.to_string_lossy().to_string()));
        let exe = exe.unwrap_or_else(|| "EzooDownload.exe".into());
        let _ = std::process::Command::new(swiftget_engine::system::windows_tool("taskkill.exe"))
            .args(["/F", "/PID", &pid.to_string(), "/FI", &format!("IMAGENAME eq {exe}")])
            .creation_flags(0x0800_0000) // no console window
            .status();
    }
    #[cfg(not(windows))]
    let _ = pid;
}

fn run() -> anyhow::Result<()> {
    // started by Windows at sign-in: stay next to the clock, no window
    let background = std::env::args().any(|a| a == "--background");
    let data_dir = default_data_dir();
    let port = Settings::load(&data_dir.join("settings.json")).browser_port;
    if std::env::args().any(|a| a == "--quit") {
        // run by the setup: the open copy closes (saving everything), this one too
        api::notify_quit(port);
        return Ok(());
    }
    note(&format!(
        "start{}{}",
        if background { " (with Windows)" } else { "" },
        if elevated() { " (as administrator)" } else { "" }
    ));

    // opened again by the program itself: the old one may still be closing
    let mut first = api::bind(port);
    if first.is_err() && std::env::args().any(|a| a == "--restarted") {
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(250));
            first = api::bind(port);
            if first.is_ok() {
                break;
            }
        }
    }
    // a .torrent file / magnet link opened with Ezoo Download (double click)
    let open_torrent = std::env::args().skip(1).find(|a| api::torrent_source_ok(a));
    // One instance only: if the port is taken by us, bring that one to front.
    let server = match first {
        Ok(s) => Some(s),
        Err(_) => {
            if let Some(src) = &open_torrent {
                if api::notify_open_torrent(port, src) {
                    return Ok(());
                }
            }
            match api::notify_existing(port) {
                api::Existing::Shown => {
                    note("already open: brought to the front");
                    return Ok(());
                }
                api::Existing::Hangs(pid) => {
                    note(&format!("already open but not answering (process {pid})"));
                    let yes = matches!(
                        rfd::MessageDialog::new()
                            .set_title(APP)
                            .set_level(rfd::MessageLevel::Warning)
                            .set_description(
                                "Ezoo Download is already open but does not answer.\nClose it and open it again?\n\n\
                                 Ezoo Download مفتوح بالفعل لكنه لا يستجيب.\nهل تريد إغلاقه وفتحه من جديد؟",
                            )
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show(),
                        rfd::MessageDialogResult::Yes
                    );
                    if !yes || pid == 0 || pid == std::process::id() {
                        return Ok(());
                    }
                    end_process(pid);
                    // its port is free a moment later
                    let mut got = None;
                    for _ in 0..40 {
                        std::thread::sleep(Duration::from_millis(250));
                        if let Ok(s) = api::bind(port) {
                            got = Some(s);
                            break;
                        }
                    }
                    got
                }
                api::Existing::None => None,
            }
        }
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?;
    let mgr = Manager::new(rt.handle().clone(), data_dir.clone())?;
    // yt-dlp and ffmpeg: put back if missing, update silently once a day
    swiftget_engine::tools::spawn_updater(rt.handle(), mgr.client(), data_dir.clone());

    let main = MainWindow::new()?;
    let add = AddDialog::new()?;
    let batch = BatchDialog::new()?;
    let settings_dlg = SettingsDialog::new()?;
    let complete = CompleteDialog::new()?;
    let columns = ColumnsDialog::new()?;
    let category = CategoryDialog::new()?;
    let find = FindDialog::new()?;
    let props = PropertiesDialog::new()?;
    let dup = DuplicateDialog::new()?;
    let countdown = CountdownDialog::new()?;
    let sched = SchedulerDialog::new()?;
    let grabber = GrabberDialog::new()?;
    let tips = TipDialog::new()?;
    let refresh_dlg = RefreshDialog::new()?;
    let playlist_dlg = PlaylistDialog::new()?;
    let torrent_dlg = TorrentDialog::new()?;
    let browsers_dlg = BrowserDialog::new()?;
    let updater_dlg = UpdateDialog::new()?;
    let about_dlg = AboutDialog::new()?;
    {
        about_dlg.on_open_link(|u| {
            if u.starts_with("https://") {
                let _ = open::that_detached(u.as_str());
            }
        });
        let w = about_dlg.as_weak();
        about_dlg.on_close_window(move || {
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
        });
        ABOUT.with(|a| *a.borrow_mut() = Some(about_dlg.as_weak()));
    }

    let ctx = Ctx {
        mgr: mgr.clone(),
        rt: rt.handle().clone(),
        main: main.as_weak(),
        add: add.as_weak(),
        add_state: Arc::new(Mutex::new(AddState::default())),
    };
    let ctx_exit = ctx.clone();
    let ui = Ui {
        ctx: ctx.clone(),
        rows: Rc::new(VecModel::default()),
        queues: Rc::new(VecModel::default()),
        cats: Rc::new(VecModel::default()),
        progress: Rc::new(RefCell::new(HashMap::new())),
        batch: batch.as_weak(),
        settings_dlg: settings_dlg.as_weak(),
        complete: complete.as_weak(),
        complete_id: Rc::new(Cell::new(None)),
        columns: columns.as_weak(),
        category: category.as_weak(),
        editing_category: Rc::new(RefCell::new(None)),
        find: find.as_weak(),
        props: props.as_weak(),
        props_id: Rc::new(Cell::new(None)),
        dup: dup.as_weak(),
        pending: Rc::new(RefCell::new(None)),
        countdown: countdown.as_weak(),
        power: Rc::new(RefCell::new(None)),
        sched: sched.as_weak(),
        sched_state: Rc::new(RefCell::new(scheduler::SchedState::default())),
        grabber: grabber.as_weak(),
        grab_state: Rc::new(RefCell::new(grab::GrabUi::default())),
        tips: tips.as_weak(),
        refresh_dlg: refresh_dlg.as_weak(),
        refresh_id: Rc::new(Cell::new(None)),
        options_lists: Rc::new(RefCell::new(options::Lists::default())),
        sel: Rc::new(RefCell::new(Selection::default())),
        playlist: playlist_dlg.as_weak(),
        playlist_rows: Rc::new(VecModel::default()),
        torrent: torrent_dlg.as_weak(),
        torrent_rows: Rc::new(VecModel::default()),
        browsers: browsers_dlg.as_weak(),
        updater: updater_dlg.as_weak(),
        columns_model: Rc::new(VecModel::default()),
        browser_list: Rc::new(RefCell::new(Vec::new())),
    };
    main.set_rows(ModelRc::from(ui.rows.clone()));
    main.set_columns(ModelRc::from(ui.columns_model.clone()));
    main.set_queues(ModelRc::from(ui.queues.clone()));
    main.set_categories(ModelRc::from(ui.cats.clone()));
    // Arabic or English (Options > General > Language, View > Language)
    set_language(&ui, i18n::effective(&mgr.settings().language));
    {
        // first run: start with Windows, like other download managers
        let mut s = mgr.settings();
        if !s.autostart_set {
            // the installer already asked; a copy run without it starts with Windows
            if !autostart::installed_by_setup() {
                autostart::set(true);
            }
            s.autostart_set = true;
            let _ = mgr.set_settings(s);
        }
    }
    let tray = tray::Tray::new();
    let has_tray = tray.is_some();
    {
        let s = mgr.settings();
        apply_theme(&ui, s.theme as i32);
        apply_view_settings(&ui, &s);
    }

    wire_main(&ui);
    // a new version of the program itself: from GitHub, installed silently
    update::spawn(&ui.ctx);
    wire_add(&ui);
    wire_batch(&ui);
    wire_complete(&ui);
    options::wire(&ui);
    scheduler::wire(&ui);
    grab::wire(&ui);
    extra::wire(&ui);
    playlist::wire(&ui);
    torrent_ui::wire(&ui);
    browser::wire(&ui);

    // ------------------------------------------------------------ browser + clipboard
    if let Some(server) = server {
        let ctx = ctx.clone();
        let cfg_mgr = mgr.clone();
        // the API thread only sends; the UI thread owns the windows
        let (tx, rx) = std::sync::mpsc::channel::<api::ApiEvent>();
        let rx = Rc::new(rx);
        api::serve(server, move || api::config_json(&cfg_mgr.settings()), move |event| {
            let _ = tx.send(event);
            let _ = slint::invoke_from_event_loop(|| {});
        });
        let ui2 = ui.clone();
        let api_timer = Timer::default();
        api_timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
            if modal_open() {
                return; // the requests wait until the message box is closed
            }
            while let Ok(ev) = rx.try_recv() {
                handle_api(&ui2, ev);
            }
        });
        std::mem::forget(api_timer);
        let _ = ctx;
    }
    {
        let enabled_mgr = mgr.clone();
        let ctx = ctx.clone();
        let types_mgr = mgr.clone();
        clipboard::spawn(
            move || enabled_mgr.settings().clipboard_monitor,
            move || {
                types_mgr
                    .settings()
                    .capture_exts
                    .split_whitespace()
                    .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
                    .filter(|e| !e.is_empty() && !e.contains('*'))
                    .collect()
            },
            move |url| {
                let ctx = ctx.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    // a torrent always asks first (it also shares with others)
                    if ctx.mgr.settings().auto_start_clipboard && !swiftget_engine::torrent::is_magnet(&url) {
                        if ctx.mgr.find_url(&url).is_none() {
                            ctx.mgr.add(NewDownload { url, start: true, ..Default::default() });
                        }
                        return;
                    }
                    let busy = ctx.add.upgrade().map(|d| d.window().is_visible()).unwrap_or(true);
                    if !busy {
                        open_add_dialog(&ctx, url, RequestHeaders::default(), None, VideoExtra::default());
                    }
                });
            },
        );
    }

    // ------------------------------------------------------------ refresh loops
    // progress windows: 10 times a second, so bars and numbers move smoothly
    let fast_timer = Timer::default();
    {
        let ui = ui.clone();
        let ticks = Cell::new(0u32);
        fast_timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            if modal_open() {
                return;
            }
            if !ui.progress.borrow().is_empty() {
                update_progress_windows(&ui);
            }
            grab::poll(&ui);
            ticks.set(ticks.get().wrapping_add(1));
            if let Some(t) = &tray {
                // the check mark only needs to follow about once a second
                if ticks.get().is_multiple_of(10) {
                    t.set_limiter(ui.ctx.mgr.settings().speed_limit_on);
                }
                for action in t.poll() {
                    tray_action(&ui, action);
                }
                for (id, action) in tray::take_download_actions() {
                    match action {
                        tray::DownloadAction::Show => open_progress(&ui, id),
                        tray::DownloadAction::Pause => ui.ctx.mgr.pause(id),
                    }
                }
            }
        });
    }
    // the list: a few times a second
    let timer = Timer::default();
    {
        let ui = ui.clone();
        let hidden_ticks = Cell::new(0u32);
        timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            // the windows are alive (a second copy started by the user asks)
            api::UI_ALIVE.store(api::now_secs(), std::sync::atomic::Ordering::Relaxed);
            if modal_open() {
                return;
            }
            // a file picker that was open is closed again
            api::UI_WAITS.store(false, std::sync::atomic::Ordering::Relaxed);
            // next to the clock (window hidden): the list is brought up to
            // date only every few seconds, nobody looks at it
            let shown = ui.ctx.main.upgrade().map(|m| m.window().is_visible() && !m.window().is_minimized()).unwrap_or(false);
            hidden_ticks.set(if shown { 0 } else { hidden_ticks.get().wrapping_add(1) });
            if shown || hidden_ticks.get() % 12 == 1 {
                refresh(&ui);
            }
            scheduler::poll(&ui);

            // the download behind an open "Download File Info" says nothing
            // until Start is pressed
            let early = early_download(&ui.ctx);
            let finished: Vec<u64> = ui.ctx.mgr.take_completed().into_iter().filter(|id| Some(*id) != early).collect();
            if let Some(&last) = finished.last() {
                if ui.ctx.mgr.settings().show_complete_dialog {
                    show_complete(&ui, last);
                }
            }
            handle_events(&ui);
            if let Some(action) = ui.ctx.mgr.take_power_request() {
                extra::start_countdown(&ui, action);
            }
            if ui.ctx.mgr.take_exit_request() {
                let _ = slint::quit_event_loop();
            }
        });
    }
    // shut down / sleep countdown: once a second
    let power_timer = Timer::default();
    {
        let ui = ui.clone();
        power_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            extra::tick_countdown(&ui);
            follow_sizes(&ui);
            // the window may have been shown again from the icon next to the clock
            if let Some(m) = ui.ctx.main.upgrade() {
                if m.window().is_visible() {
                    winfx::set_opacity(m.window(), ui.ctx.mgr.settings().opacity);
                }
            }
        });
    }
    refresh(&ui);

    {
        let mgr = mgr.clone();
        main.window().on_close_requested(move || {
            // with the icon next to the clock, closing only hides the window
            if !(has_tray && mgr.settings().close_to_tray) {
                let _ = slint::quit_event_loop();
            }
            slint::CloseRequestResponse::HideWindow
        });
    }
    // after an update: what is new, once, in the language of the program
    let mut just_updated = false;
    {
        let mut s = mgr.settings();
        let now = env!("CARGO_PKG_VERSION");
        if s.last_version != now {
            // an empty last_version on a copy used before = an update from 1.0.3
            let updated = if s.last_version.is_empty() { s.extension_prompted } else { true };
            s.last_version = now.into();
            let _ = mgr.set_settings(s);
            just_updated = updated;
            if updated {
                let ui = ui.clone();
                Timer::single_shot(Duration::from_millis(1500), move || extra::whats_new(&ui));
            }
        }
    }
    // the size the user gave the main window last time
    if let Some((w, h)) = saved_size(&mgr, "main") {
        main.window().set_size(slint::LogicalSize::new(w, h));
    }
    let was_maximized = mgr.settings().main_maximized;
    if !(background && has_tray) {
        main.show()?;
        // the main window: in the middle of the screen, and it can be maximized
        winfx::place(main.as_weak(), true);
        if was_maximized {
            let w = main.as_weak();
            Timer::single_shot(Duration::from_millis(60), move || {
                if let Some(m) = w.upgrade() {
                    m.window().set_maximized(true);
                }
            });
        }
        winfx::set_opacity(main.window(), mgr.settings().opacity);
        let mut s = mgr.settings();
        if !s.extension_prompted {
            // first start: help adding the browser extension
            s.extension_prompted = true;
            let _ = mgr.set_settings(s);
            let ui = ui.clone();
            Timer::single_shot(Duration::from_millis(800), move || browser::open(&ui));
        } else if s.show_tips && !just_updated {
            // never together with a message box (What's new, a question…)
            let ui = ui.clone();
            Timer::single_shot(Duration::from_millis(600), move || extra::show_tip_when_free(ui));
        }
    }
    if let Some(src) = open_torrent {
        // opened from Windows with a torrent: its window, once everything is up
        Timer::single_shot(Duration::from_millis(500), move || torrent_ui::open_source(&src));
    }
    {
        // a .torrent file dropped on the window
        winfx::on_files_dropped(main.window(), |path| {
            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("torrent")) && path.is_file() {
                torrent_ui::open_source(&path.display().to_string());
            }
        });
    }
    slint::run_event_loop_until_quit()?;
    note("closed");
    preview::clean_all();
    {
        // opens maximized next time when it was maximized now
        let max = main.window().is_maximized();
        let mut s = mgr.settings();
        if s.main_maximized != max {
            s.main_maximized = max;
            let _ = mgr.set_settings(s);
        }
    }

    // save everything and stop downloads cleanly (they resume next time)
    drop(timer);
    drop(fast_timer);
    drop(power_timer);
    // a download nobody pressed Start for is not kept
    cancel_early(&ctx_exit);
    mgr.shutdown();
    rt.shutdown_timeout(Duration::from_secs(2));
    if RESTART.load(std::sync::atomic::Ordering::SeqCst) {
        restart_now();
    }
    Ok(())
}

// ===================================================================== main window

fn wire_main(ui: &Ui) {
    let Some(main) = ui.ctx.main.upgrade() else { return };
    let mgr = ui.ctx.mgr.clone();
    {
        let ctx = ui.ctx.clone();
        main.on_add_url(move || {
            let url = arboard::Clipboard::new()
                .ok()
                .and_then(|mut c| c.get_text().ok())
                .and_then(|t| clipboard::downloadable_url(&t).or_else(|| is_http_url(&t)))
                .unwrap_or_default();
            open_add_dialog(&ctx, url, RequestHeaders::default(), None, VideoExtra::default());
        });
    }
    {
        let ui = ui.clone();
        main.on_add_batch(move || open_batch(&ui, String::new()));
    }
    {
        let ui = ui.clone();
        main.on_add_batch_clipboard(move || {
            let text = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()).unwrap_or_default();
            open_batch(&ui, text);
        });
    }
    {
        let ui = ui.clone();
        main.on_resume(move |id| {
            let targets = targets(&ui, id as u64);
            for &t in &targets {
                ui.ctx.mgr.start(t);
            }
            if targets.len() <= 8 {
                for &t in &targets {
                    show_progress_for_start(&ui, t);
                }
            }
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_pause(move |id| {
            for t in targets(&ui, id as u64) {
                ui.ctx.mgr.pause(t);
            }
            refresh(&ui);
        });
    }
    {
        let mgr = mgr.clone();
        main.on_resume_all(move || mgr.start_all());
    }
    {
        let mgr = mgr.clone();
        main.on_stop_all(move || mgr.pause_all());
    }
    {
        let ui = ui.clone();
        main.on_delete(move |id| {
            // not inside the key press / click itself: the message box runs
            // its own message loop (holding Delete down asked many times)
            if modal_open() {
                return;
            }
            let ids = targets(&ui, id as u64);
            let ui = ui.clone();
            Timer::single_shot(Duration::from_millis(1), move || {
                if !modal_open() {
                    delete_downloads(&ui, ids);
                    refresh(&ui);
                }
            });
        });
    }
    {
        let ui = ui.clone();
        main.on_delete_completed(move || {
            let ui2 = ui.clone();
            confirm(&ui.ctx, APP, &t("Remove every finished download from the list?\n(The files stay on the disk.)"), move || {
                ui2.ctx.mgr.remove_completed();
                refresh(&ui2);
            });
        });
    }
    {
        let ui = ui.clone();
        main.on_view_changed(move |_| refresh(&ui));
    }
    {
        let ui = ui.clone();
        main.on_search_changed(move || refresh(&ui));
    }
    {
        let ui = ui.clone();
        main.on_open_settings(move |tab| options::open(&ui, tab));
    }
    {
        let ui = ui.clone();
        main.on_open_limiter(move || options::open(&ui, 8));
    }
    {
        let ui = ui.clone();
        main.on_open_scheduler(move || scheduler::open(&ui, None));
    }
    {
        let ui = ui.clone();
        main.on_toggle_limiter(move || toggle_limiter(&ui));
    }
    {
        let ui = ui.clone();
        main.on_sort(move |key| {
            let mut s = ui.ctx.mgr.settings();
            if s.sort_by == key.as_str() {
                s.sort_desc = !s.sort_desc;
            } else {
                s.sort_by = key.to_string();
                s.sort_desc = matches!(key.as_str(), "date" | "size" | "added");
            }
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_sort_by(s.sort_by.clone().into());
                m.set_sort_desc(s.sort_desc);
            }
            let _ = ui.ctx.mgr.set_settings(s);
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_set_theme(move |m| {
            let mut s = ui.ctx.mgr.settings();
            s.theme = m.clamp(0, 8) as u32;
            let _ = ui.ctx.mgr.set_settings(s);
            apply_theme(&ui, m);
        });
    }
    {
        let ui = ui.clone();
        main.on_set_language(move |lang| {
            let mut s = ui.ctx.mgr.settings();
            s.language = lang.to_string();
            let _ = ui.ctx.mgr.set_settings(s);
            change_language(&ui, &lang);
        });
    }
    {
        let ui = ui.clone();
        main.on_toggle_categories(move || {
            let mut s = ui.ctx.mgr.settings();
            s.hide_categories = !s.hide_categories;
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_hide_categories(s.hide_categories);
            }
            let _ = ui.ctx.mgr.set_settings(s);
        });
    }
    {
        let ui = ui.clone();
        main.on_set_toolbar_small(move |small| {
            let mut s = ui.ctx.mgr.settings();
            s.toolbar_small = small;
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_toolbar_small(small);
            }
            let _ = ui.ctx.mgr.set_settings(s);
        });
    }
    {
        let ui = ui.clone();
        main.on_start_queue(move |q| {
            ui.ctx.mgr.start_queue(q as u64);
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_stop_queue(move |q| {
            ui.ctx.mgr.stop_queue(q as u64);
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_new_queue(move || scheduler::new_queue(&ui));
    }
    {
        let ui = ui.clone();
        main.on_edit_queue(move |q| scheduler::open(&ui, Some(q as u64)));
    }
    {
        let ui = ui.clone();
        main.on_delete_queue(move |q| scheduler::delete_queue(&ui, q as u64));
    }
    {
        let ui = ui.clone();
        main.on_row_activated(move |id| {
            let Some(v) = ui.ctx.mgr.get(id as u64) else { return };
            if ui.ctx.mgr.settings().double_click == 1 {
                extra::open_properties(&ui, v.id);
            } else if v.status == Status::Completed {
                open_download(&v.save_path);
            } else {
                open_progress(&ui, v.id);
            }
        });
    }
    {
        let ui = ui.clone();
        main.on_row_action(move |id, action| {
            let targets = targets(&ui, id as u64);
            match action.as_str() {
                // these work on every selected download
                "resume" | "stop" | "restart" | "delete" | "copy" if targets.len() > 1 => {
                    multi_action(&ui, &targets, action.as_str())
                }
                a => row_action(&ui, id as u64, a),
            }
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_row_to_queue(move |id, q| {
            for t in targets(&ui, id as u64) {
                ui.ctx.mgr.set_queue(t, (q > 0).then_some(q as u64));
            }
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_row_clicked(move |id, ctrl, shift| {
            row_clicked(&ui, id as u64, ctrl, shift);
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_row_right(move |id| {
            if !ui.sel.borrow().ids.contains(&(id as u64)) {
                select_single(&ui, id as u64);
            }
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_selected_id(id);
            }
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        main.on_row_drag(move |id, offset| {
            // select with the mouse: from the row where the drag started
            let order = visible_ids(&ui);
            let Some(start) = order.iter().position(|x| *x == id as u64) else { return };
            let end = (start as i64 + offset as i64).clamp(0, order.len() as i64 - 1) as usize;
            let mut sel = ui.sel.borrow_mut();
            sel.ids = order[start.min(end)..=start.max(end)].to_vec();
            sel.anchor = Some(id as u64);
            drop(sel);
            refresh(&ui);
        });
    }
    {
        // the header: drag an edge to resize a column (live), let go to keep it
        let ui = ui.clone();
        main.on_column_resized(move |i, w| {
            if let Some(mut c) = ui.columns_model.row_data(i.max(0) as usize) {
                c.width = w.clamp(36.0, 2000.0);
                ui.columns_model.set_row_data(i as usize, c);
            }
        });
    }
    {
        let ui = ui.clone();
        main.on_columns_changed(move || save_columns(&ui));
    }
    {
        // the header: a title dragged onto another place moves the column there
        let ui = ui.clone();
        main.on_column_dropped(move |from, x| {
            let widths: Vec<f32> = (0..ui.columns_model.row_count()).filter_map(|i| ui.columns_model.row_data(i)).map(|c| c.width).collect();
            let to = column_at(&widths, x);
            let from = from.max(0) as usize;
            if from < widths.len() && to != from {
                if let Some(c) = ui.columns_model.row_data(from) {
                    ui.columns_model.remove(from);
                    ui.columns_model.insert(to.min(ui.columns_model.row_count()), c);
                    save_columns(&ui);
                }
            }
        });
    }
    {
        let ui = ui.clone();
        main.on_row_drag_out(move |id| {
            // a finished file pulled out of the list: Windows takes it from here
            let Some(v) = ui.ctx.mgr.get(id as u64) else { return };
            if v.status != Status::Completed || !v.save_path.is_file() {
                return;
            }
            let Some(m) = ui.ctx.main.upgrade() else { return };
            winfx::drag_file_out(m.window(), &v.save_path);
            // the mouse was let go outside the list: the row must know too
            m.window().dispatch_event(slint::platform::WindowEvent::PointerReleased {
                position: slint::LogicalPosition::new(0.0, 0.0),
                button: slint::platform::PointerEventButton::Left,
            });
            m.window().dispatch_event(slint::platform::WindowEvent::PointerExited);
        });
    }
    {
        let ui = ui.clone();
        main.on_select_all(move || {
            let ids = visible_ids(&ui);
            let mut sel = ui.sel.borrow_mut();
            sel.anchor = ids.first().copied();
            sel.ids = ids;
            drop(sel);
            refresh(&ui);
        });
    }
    {
        let mgr = mgr.clone();
        main.on_open_download_folder(move || {
            let dir = mgr.settings().download_dir;
            let _ = std::fs::create_dir_all(&dir);
            let _ = open::that_detached(&dir);
        });
    }
    {
        let mgr = mgr.clone();
        main.on_export_list(move |kind| extra::export_list(&mgr, kind.as_str()));
    }
    {
        let ui = ui.clone();
        main.on_import_list(move |kind| extra::import_list(&ui, kind.as_str()));
    }
    {
        let ui = ui.clone();
        main.on_set_all_done(move |i| {
            ui.ctx.mgr.set_when_all_done(menu_done_from(i));
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_all_done(i);
            }
        });
    }
    {
        let ui = ui.clone();
        main.on_open_columns(move || extra::open_columns(&ui));
    }
    {
        let ui = ui.clone();
        main.on_add_category(move || extra::open_category(&ui, None));
    }
    {
        let ui = ui.clone();
        main.on_category_props(move |name| extra::open_category(&ui, Some(name.to_string())));
    }
    {
        let ui = ui.clone();
        main.on_delete_category(move |name| extra::delete_category(&ui, name.as_str()));
    }
    {
        let ui = ui.clone();
        main.on_find(move || extra::open_find(&ui));
    }
    {
        let ui = ui.clone();
        main.on_find_next(move || extra::find_next(&ui));
    }
    {
        let ui = ui.clone();
        main.on_grabber(move || grab::open_grabber(&ui));
    }
    {
        let ui = ui.clone();
        main.on_tips(move || extra::show_tip(&ui, true));
    }
    {
        let ui = ui.clone();
        main.on_playlist(move || playlist::open(&ui));
    }
    {
        let ui = ui.clone();
        main.on_torrent(move || torrent_ui::open(&ui, None));
    }
    {
        let ui = ui.clone();
        main.on_browsers(move || browser::open(&ui));
    }
    let (tools_client, tools_rt, tools_dir) = (mgr.client(), ui.ctx.rt.clone(), default_data_dir());
    main.on_update_tools(move || {
        // ffmpeg too, right now (removing the stamps makes it check)
        let _ = std::fs::remove_file(tools_dir.join("ffmpeg-checked"));
        tools_rt.spawn(swiftget_engine::tools::update_all(tools_client.clone(), tools_dir.clone()));
        std::thread::spawn(|| {
            let text = match swiftget_engine::ytdlp::update_now() {
                Ok(out) => tf(
                    "Ezoo Download {}\n\nThe video downloader (yt-dlp) is up to date.\n\n{}",
                    &[&env!("CARGO_PKG_VERSION"), &out],
                ),
                Err(e) => tf("Could not update yt-dlp:\n\n{}", &[&format!("{e:#}")]),
            };
            let _ = slint::invoke_from_event_loop(move || message(&text));
        });
    });
    {
        let ui2 = ui.clone();
        main.on_check_app_update(move || update::check_now(&ui2));
    }
    if let Some(d) = ui.updater.upgrade() {
        let w = d.as_weak();
        d.on_close_window(move || {
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
        });
    }
    main.on_about(about);
    main.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
}

/// Ids in the order the list shows them.
fn visible_ids(ui: &Ui) -> Vec<u64> {
    (0..ui.rows.row_count()).filter_map(|i| ui.rows.row_data(i)).map(|r| r.id as u64).collect()
}

pub(crate) fn select_single(ui: &Ui, id: u64) {
    let mut sel = ui.sel.borrow_mut();
    sel.ids = vec![id];
    sel.anchor = Some(id);
    drop(sel);
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_selected_id(id as i32);
    }
}

/// Click on a row: alone, Ctrl = add / remove, Shift = range from the last click.
fn row_clicked(ui: &Ui, id: u64, ctrl: bool, shift: bool) {
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_selected_id(id as i32);
    }
    let order = visible_ids(ui);
    let mut sel = ui.sel.borrow_mut();
    if shift {
        let anchor = sel.anchor.filter(|a| order.contains(a)).unwrap_or(id);
        let (a, b) = (
            order.iter().position(|x| *x == anchor).unwrap_or(0),
            order.iter().position(|x| *x == id).unwrap_or(0),
        );
        let range: Vec<u64> = order[a.min(b)..=a.max(b)].to_vec();
        if ctrl {
            for r in range {
                if !sel.ids.contains(&r) {
                    sel.ids.push(r);
                }
            }
        } else {
            sel.ids = range;
        }
    } else if ctrl {
        if let Some(p) = sel.ids.iter().position(|x| *x == id) {
            sel.ids.remove(p);
        } else {
            sel.ids.push(id);
        }
        sel.anchor = Some(id);
    } else {
        sel.ids = vec![id];
        sel.anchor = Some(id);
    }
}

/// The downloads an action works on: the whole selection when the clicked
/// one is part of it, otherwise only that one.
fn targets(ui: &Ui, id: u64) -> Vec<u64> {
    let sel = ui.sel.borrow();
    if id as i64 >= 0 && sel.ids.contains(&id) && sel.ids.len() > 1 {
        let order = visible_ids(ui);
        let mut v: Vec<u64> = order.into_iter().filter(|x| sel.ids.contains(x)).collect();
        if v.is_empty() {
            v = sel.ids.clone();
        }
        v
    } else if (id as i64) >= 0 {
        vec![id]
    } else {
        Vec::new()
    }
}

/// Resume / stop / redownload / delete / copy several downloads at once.
fn multi_action(ui: &Ui, ids: &[u64], action: &str) {
    match action {
        "resume" => {
            for &i in ids.iter() {
                ui.ctx.mgr.start(i);
            }
            // a progress window for each (not for a long list at once)
            if ids.len() <= 8 {
                for &i in ids.iter() {
                    show_progress_for_start(ui, i);
                }
            }
        }
        "stop" => ids.iter().for_each(|&i| ui.ctx.mgr.pause(i)),
        "restart" => {
            if ask(&tf("Download these {} files again from the beginning?", &[&ids.len()])) {
                ids.iter().for_each(|&i| ui.ctx.mgr.restart(i));
            }
        }
        "delete" => {
            if modal_open() {
                return;
            }
            let (ui, ids) = (ui.clone(), ids.to_vec());
            Timer::single_shot(Duration::from_millis(1), move || {
                if !modal_open() {
                    delete_downloads(&ui, ids);
                    refresh(&ui);
                }
            });
        }
        "copy" => {
            let text: Vec<String> = ids.iter().filter_map(|&i| ui.ctx.mgr.get(i)).map(|v| v.url).collect();
            if let Ok(mut c) = arboard::Clipboard::new() {
                let _ = c.set_text(text.join("\r\n"));
            }
        }
        _ => {}
    }
}

fn about() {
    ABOUT.with(|a| {
        if let Some(d) = a.borrow().as_ref().and_then(|w| w.upgrade()) {
            d.set_version(env!("CARGO_PKG_VERSION").into());
            open_window!(d);
        }
    });
}

thread_local! {
    static ABOUT: RefCell<Option<Weak<AboutDialog>>> = const { RefCell::new(None) };
}

fn tray_action(ui: &Ui, action: tray::TrayAction) {
    use tray::TrayAction as T;
    match action {
        T::Show => bring_to_front(&ui.ctx),
        T::AddUrl => {
            bring_to_front(&ui.ctx);
            open_add_dialog(&ui.ctx, String::new(), RequestHeaders::default(), None, VideoExtra::default());
        }
        T::ResumeAll => ui.ctx.mgr.start_all(),
        T::StopAll => ui.ctx.mgr.pause_all(),
        T::ToggleLimiter => toggle_limiter(ui),
        T::Scheduler => scheduler::open(ui, None),
        T::Options => options::open(ui, 0),
        T::About => about(),
        T::Exit => {
            let _ = slint::quit_event_loop();
        }
    }
}

fn toggle_limiter(ui: &Ui) {
    let mut s = ui.ctx.mgr.settings();
    s.speed_limit_on = !s.speed_limit_on;
    if s.speed_limit_kb == 0 {
        s.speed_limit_kb = 500;
    }
    let _ = ui.ctx.mgr.set_settings(s);
    refresh(ui);
}

/// Sounds and messages for what the download engine reports.
fn handle_events(ui: &Ui) {
    let events = ui.ctx.mgr.take_events();
    if events.is_empty() {
        return;
    }
    let s = ui.ctx.mgr.settings();
    let early = early_download(&ui.ctx);
    // a finished file: the preview names of it are not needed any more
    preview::clean_up();
    for ev in events {
        match ev {
            Event::Completed(id) | Event::Failed(id) if Some(id) == early => {}
            Event::Completed(_) if s.sounds_on => sound::play(&s.sound_complete),
            Event::Failed(_) if s.sounds_on => sound::play(&s.sound_failed),
            Event::QueueStarted(_) if s.sounds_on => sound::play(&s.sound_queue_start),
            Event::QueueDone(_) if s.sounds_on => sound::play(&s.sound_queue_done),
            Event::QuotaReached if s.quota_warn => message(&tf(
                "The download limit of {} MB every {} hours was reached, so the downloads were stopped.\nThey continue by themselves when the limit allows it again (Options > Connection).",
                &[&s.quota_mb, &s.quota_hours],
            )),
            Event::OpenFile(id) => {
                if let Some(v) = ui.ctx.mgr.get(id) {
                    open_download(&v.save_path);
                }
            }
            _ => {}
        }
    }
}

thread_local! {
    static LAST_API_ADD: RefCell<Option<(String, std::time::Instant)>> = const { RefCell::new(None) };
}

/// A request from the browser extension.
fn handle_api(ui: &Ui, event: api::ApiEvent) {
    match event {
        api::ApiEvent::Show => bring_to_front(&ui.ctx),
        api::ApiEvent::Quit => {
            note("closed for the setup");
            let _ = slint::quit_event_loop();
        }
        api::ApiEvent::OpenTorrent(s) => torrent_ui::open_source(&s),
        api::ApiEvent::Playlist(url) => playlist::open_url(ui, &url),
        api::ApiEvent::Links(l) => grab::open_links(ui, l),
        api::ApiEvent::Add(req) => {
            // the same address again within a few seconds (an older browser
            // add-in could send a download over and over): show nothing new
            let repeat = LAST_API_ADD.with(|l| {
                let mut l = l.borrow_mut();
                let now = std::time::Instant::now();
                let same = l.as_ref().map(|(u, at)| *u == req.url && now.duration_since(*at) < Duration::from_secs(10)).unwrap_or(false);
                *l = Some((req.url.clone(), now));
                same
            });
            if repeat {
                return;
            }
            let mut headers = RequestHeaders {
                referer: req.referrer,
                cookies: req.cookies,
                user_agent: req.user_agent,
                extra: Vec::new(),
                auth: None,
            };
            if let Some(list) = &req.headers {
                swiftget_engine::ytdlp::apply_headers(&mut headers, list);
            }
            // a torrent's magnet link from a page: the torrent window asks first
            if swiftget_engine::torrent::is_magnet(&req.url) {
                torrent_ui::open_source(&req.url);
                return;
            }
            // a .torrent file's link: straight to the torrent window too
            if torrent_ui::looks_like_torrent(&req.url, req.file_name.as_deref()) {
                torrent_ui::fetch_and_open(&ui.ctx, req.url, headers, req.file_name);
                return;
            }
            // "Refresh download address": this is the new link we waited for
            if let Some(id) = ui.refresh_id.get() {
                if req.audio_url.is_none() {
                    extra::refresh_received(ui, id, &req.url, headers);
                    return;
                }
            }
            let extra = VideoExtra {
                audio_url: req.audio_url,
                source_page: req.source_page,
                source_key: req.source_key,
            };
            let s = ui.ctx.mgr.settings();
            if !s.show_start_dialog && ui.ctx.mgr.find_url(&req.url).is_none() {
                // straight to the list, without the "Download File Info" window
                let id = ui.ctx.mgr.add(NewDownload {
                    url: req.url,
                    file_name: req.file_name,
                    headers,
                    audio_url: extra.audio_url,
                    source_page: extra.source_page,
                    source_key: extra.source_key,
                    start: true,
                    ..Default::default()
                });
                show_progress_for_start(ui, id);
                refresh(ui);
                return;
            }
            open_add_dialog(&ui.ctx, req.url, headers, req.file_name, extra);
        }
    }
}

// ===================================================================== download file info

fn wire_add(ui: &Ui) {
    let Some(add) = ui.ctx.add.upgrade() else { return };
    {
        let ctx = ui.ctx.clone();
        add.on_check_url(move || {
            if let Some(d) = ctx.add.upgrade() {
                let url = d.get_url().trim().to_string();
                // another address: the download started for the old one stops
                if url != ctx.add_state.lock().unwrap().origin_url {
                    cancel_early(&ctx);
                }
                if swiftget_engine::torrent::is_magnet(&url) {
                    // typed / pasted a magnet link: that is the torrent window's job
                    let _ = d.hide();
                    torrent_ui::open_source(&url);
                    return;
                }
                if !url.is_empty() {
                    start_probe(&ctx, url);
                }
            }
        });
    }
    {
        let ctx = ui.ctx.clone();
        add.on_category_changed(move |i| {
            let Some(d) = ctx.add.upgrade() else { return };
            let cat = ctx
                .add_state
                .lock()
                .unwrap()
                .categories
                .get(i.max(0) as usize)
                .cloned()
                .unwrap_or_else(|| "General".into());
            let current = PathBuf::from(d.get_save_as().as_str());
            let name = current
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "download".into());
            let path = ctx.mgr.settings().dir_for(&cat).join(&name).display().to_string();
            d.set_save_as(path.clone().into());
            d.set_category_name(t(&cat).into());
            ctx.add_state.lock().unwrap().auto_path = path;
        });
    }
    {
        let w = add.as_weak();
        add.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            let current = PathBuf::from(d.get_save_as().as_str());
            let mut dlg = crate::winfx::file_box();
            if let Some(dir) = current.parent().filter(|p| p.exists()) {
                dlg = dlg.set_directory(dir);
            }
            if let Some(name) = current.file_name() {
                dlg = dlg.set_file_name(name.to_string_lossy());
            }
            // Windows adds the file's own extension when a name is typed without it
            if let Some(ext) = current.extension().map(|e| e.to_string_lossy().into_owned()).filter(|e| !e.is_empty()) {
                dlg = dlg.add_filter(ext.to_ascii_uppercase(), &[ext.as_str()]).add_filter("*", &["*"]);
            }
            if let Some(p) = dlg.save_file() {
                let suggested = current.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let name = p.file_name().map(|n| with_extension_of(&n.to_string_lossy(), &suggested)).unwrap_or_default();
                let p = p.with_file_name(name);
                d.set_save_as(p.display().to_string().into());
            }
        });
    }
    {
        let ui = ui.clone();
        add.on_start_now(move || confirm_add(&ui, true));
    }
    {
        // the server refused us: give the address back to the browser
        let ctx = ui.ctx.clone();
        add.on_use_browser(move || {
            let Some(d) = ctx.add.upgrade() else { return };
            let url = d.get_url().trim().to_string();
            ctx.add_state.lock().unwrap().probe_seq += 1;
            cancel_early(&ctx);
            let _ = d.hide();
            if url.starts_with("http://") || url.starts_with("https://") {
                api::leave_to_browser(&url);
                let _ = open::that_detached(&url);
            }
            open_next(&ctx);
        });
    }
    {
        let ui = ui.clone();
        add.on_later(move || confirm_add(&ui, false));
    }
    {
        let ctx = ui.ctx.clone();
        add.on_cancel(move || {
            ctx.add_state.lock().unwrap().probe_seq += 1; // ignore a check still running
            cancel_early(&ctx);
            if let Some(d) = ctx.add.upgrade() {
                let _ = d.hide();
            }
            open_next(&ctx);
        });
    }
    {
        let ctx = ui.ctx.clone();
        add.window().on_close_requested(move || {
            ctx.add_state.lock().unwrap().probe_seq += 1;
            cancel_early(&ctx);
            open_next(&ctx);
            slint::CloseRequestResponse::HideWindow
        });
    }
}

// ===================================================================== batch

fn wire_batch(ui: &Ui) {
    let Some(batch) = ui.batch.upgrade() else { return };
    {
        let w = batch.as_weak();
        batch.on_paste(move || {
            let Some(d) = w.upgrade() else { return };
            if let Some(text) = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()) {
                let mut links = d.get_links().to_string();
                if !links.is_empty() && !links.ends_with('\n') {
                    links.push('\n');
                }
                links.push_str(text.trim());
                d.set_links(links.into());
            }
        });
    }
    {
        let (w, mgr) = (batch.as_weak(), ui.ctx.mgr.clone());
        batch.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            if let Some(dir) = crate::winfx::file_box().set_directory(mgr.settings().download_dir).pick_folder() {
                d.set_folder(dir.display().to_string().into());
            }
        });
    }
    {
        let w = batch.as_weak();
        batch.on_pattern_changed(move || {
            let Some(d) = w.upgrade() else { return };
            d.set_preview(batch_preview(&d).into());
        });
    }
    {
        let ui = ui.clone();
        batch.on_ok(move || {
            let Some(d) = ui.batch.upgrade() else { return };
            let (links, auth) = if d.get_mode() == 1 {
                match pattern_links(&d) {
                    Ok(l) => {
                        let user = d.get_user().trim().to_string();
                        (l, (!user.is_empty()).then(|| (user, d.get_password().to_string())))
                    }
                    Err(e) => {
                        d.set_message(e.into());
                        return;
                    }
                }
            } else {
                (extract_links(&d.get_links()), None)
            };
            if links.is_empty() {
                d.set_message(t("No valid http:// or https:// links").into());
                return;
            }
            let s = ui.ctx.mgr.settings();
            let queue = s.queues.get(d.get_queue_index().max(0) as usize).map(|q| q.id).unwrap_or(MAIN_QUEUE);
            let folder = d.get_folder().trim().to_string();
            for url in links {
                ui.ctx.mgr.add(NewDownload {
                    url,
                    save_dir: (!folder.is_empty()).then(|| PathBuf::from(&folder)),
                    headers: RequestHeaders { auth: auth.clone(), ..Default::default() },
                    queue: Some(queue),
                    start: false,
                    ..Default::default()
                });
            }
            if d.get_start_now() {
                ui.ctx.mgr.start_queue(queue);
            }
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_view(format!("queue:{queue}").into());
            }
            let _ = d.hide();
            refresh(&ui);
        });
    }
    {
        let w = batch.as_weak();
        batch.on_cancel(move || {
            if let Some(d) = w.upgrade() {
                let _ = d.hide();
            }
        });
    }
}

/// "img*.jpg" + from/to -> the list of addresses (numbers or letters).
fn expand_pattern(pattern: &str, letters: bool, from: &str, to: &str, size: usize) -> Result<Vec<String>, String> {
    let pattern = pattern.trim();
    if !(pattern.starts_with("http://") || pattern.starts_with("https://")) {
        return Err(t("The address must start with http:// or https://"));
    }
    if !pattern.contains('*') {
        return Err(t("Put a * in the address where the number goes"));
    }
    let mut out = Vec::new();
    if letters {
        let a = from.trim().chars().next().filter(|c| c.is_ascii_alphabetic()).ok_or_else(|| t("From must be a letter"))?;
        let b = to.trim().chars().next().filter(|c| c.is_ascii_alphabetic()).ok_or_else(|| t("To must be a letter"))?;
        let (a, b) = (a as u8, b as u8);
        if a > b {
            return Err(t("From must come before To"));
        }
        for c in a..=b {
            out.push(pattern.replace('*', &(c as char).to_string()));
        }
    } else {
        let a: u64 = from.trim().parse().map_err(|_| t("From must be a number"))?;
        let b: u64 = to.trim().parse().map_err(|_| t("To must be a number"))?;
        if a > b {
            return Err(t("From must be smaller than To"));
        }
        if b - a >= 10_000 {
            return Err(t("That's more than 10000 files"));
        }
        for n in a..=b {
            out.push(pattern.replace('*', &format!("{n:0size$}")));
        }
    }
    Ok(out)
}

fn pattern_links(d: &BatchDialog) -> Result<Vec<String>, String> {
    expand_pattern(
        &d.get_pattern(),
        d.get_letters(),
        &d.get_from_text(),
        &d.get_to_text(),
        d.get_wildcard_size().clamp(1, 10) as usize,
    )
}

fn batch_preview(d: &BatchDialog) -> String {
    match pattern_links(d) {
        Ok(l) if l.is_empty() => String::new(),
        Ok(l) => {
            let mut s = tf("{} files\n\nFirst:  {}", &[&l.len(), &l[0]]);
            if l.len() > 1 {
                s.push_str(&tf("\nSecond:  {}", &[&l[1]]));
            }
            if l.len() > 2 {
                s.push_str(&tf("\nLast:  {}", &[&l[l.len() - 1]]));
            }
            s
        }
        Err(e) => e,
    }
}

// ===================================================================== download complete

fn wire_complete(ui: &Ui) {
    let Some(complete) = ui.complete.upgrade() else { return };
    {
        let ui = ui.clone();
        complete.on_open_file(move || {
            if let Some(v) = ui.complete_id.get().and_then(|i| ui.ctx.mgr.get(i)) {
                open_download(&v.save_path);
            }
            close_complete(&ui);
        });
    }
    {
        let ui = ui.clone();
        complete.on_open_with(move || {
            if let Some(v) = ui.complete_id.get().and_then(|i| ui.ctx.mgr.get(i)) {
                open_with(&v.save_path);
            }
            close_complete(&ui);
        });
    }
    {
        let ui = ui.clone();
        complete.on_open_folder(move || {
            if let Some(v) = ui.complete_id.get().and_then(|i| ui.ctx.mgr.get(i)) {
                reveal_in_folder(&v);
            }
            close_complete(&ui);
        });
    }
    {
        let ui = ui.clone();
        complete.on_close_window(move || close_complete(&ui));
    }
}

pub(crate) fn show_complete(ui: &Ui, id: u64) {
    if let (Some(v), Some(d)) = (ui.ctx.mgr.get(id), ui.complete.upgrade()) {
        ui.complete_id.set(Some(id));
        d.set_name(v.file_name.clone().into());
        d.set_url(v.url.clone().into());
        d.set_size(v.total_size.map(fmt_size).unwrap_or_default().into());
        d.set_path(v.save_path.display().to_string().into());
        d.set_kind(kind_for(&v.file_name).into());
        d.set_ext(filename::extension(&v.file_name).to_uppercase().into());
        d.set_dont_show(false);
        open_window!(d);
    }
}

// ===================================================================== helpers

thread_local! {
    /// a Windows message box is open: its own message loop runs our timers,
    /// and a list refresh in the middle of the click that opened it could
    /// hang the program; the timers wait until it is closed
    static MODAL: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn modal_open() -> bool {
    MODAL.with(|m| m.get())
}

/// Marks a message box as open for as long as it lives.
pub(crate) struct ModalGuard(bool);

impl ModalGuard {
    pub(crate) fn new() -> ModalGuard {
        api::UI_WAITS.store(true, std::sync::atomic::Ordering::Relaxed);
        ModalGuard(MODAL.with(|m| m.replace(true)))
    }
}

impl Drop for ModalGuard {
    fn drop(&mut self) {
        let before = self.0;
        MODAL.with(|m| m.set(before));
        api::UI_WAITS.store(before, std::sync::atomic::Ordering::Relaxed);
        // waiting for the box was no hang
        api::UI_ALIVE.store(api::now_secs(), std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) fn message(text: &str) {
    let _g = ModalGuard::new();
    crate::winfx::msg_box().set_title(APP).set_description(text).show();
}

pub(crate) fn ask(text: &str) -> bool {
    use rfd::{MessageButtons, MessageDialogResult};
    let _g = ModalGuard::new();
    matches!(
        crate::winfx::msg_box().set_title(APP).set_description(text).set_buttons(MessageButtons::YesNo).show(),
        MessageDialogResult::Yes
    )
}

fn is_http_url(t: &str) -> Option<String> {
    let t = t.trim();
    (t.starts_with("http://") || t.starts_with("https://"))
        .then(|| t.to_string())
        .filter(|s| !s.contains(char::is_whitespace))
}

/// Every http(s) link in a block of text, one per line or separated by spaces.
pub(crate) fn extract_links(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|l| l.trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>' || c == ','))
        .filter(|l| (l.starts_with("http://") || l.starts_with("https://")) && url::Url::parse(l).is_ok())
        .map(|l| l.to_string())
        .collect()
}

/// File-type icon for a file name.
pub(crate) fn kind_for(name: &str) -> &'static str {
    match filename::extension(name).as_str() {
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "ts" | "3gp" | "mpg" | "mpeg" | "vob"
        | "rm" | "rmvb" | "ogv" | "asf" => "video",
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "opus" | "mid" | "mka" => "audio",
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "cab" | "arj" | "lzh" => "archive",
        "iso" | "img" | "dmg" => "disc",
        "pdf" => "pdf",
        "doc" | "docx" | "odt" | "rtf" | "epub" => "doc",
        "xls" | "xlsx" | "csv" | "ods" => "sheet",
        "ppt" | "pptx" | "odp" | "pps" => "slides",
        "txt" | "md" | "log" | "json" | "xml" => "text",
        "exe" | "msi" | "msix" | "appx" | "bat" | "jar" | "deb" | "rpm" => "exe",
        "apk" | "xapk" => "apk",
        "torrent" => "torrent",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "tif" | "tiff" | "heic" | "ico" | "psd" => "image",
        _ => "general",
    }
}

/// Sidebar icon of a category.
fn kind_of_category(name: &str) -> &'static str {
    match name {
        "Compressed" => "archive",
        "Documents" => "doc",
        "Music" => "audio",
        "Programs" => "exe",
        "Video" => "video",
        "Images" => "image",
        "General" => "general",
        _ => "folder",
    }
}

pub(crate) fn queue_names(s: &Settings) -> ModelRc<SharedString> {
    let names: Vec<SharedString> = s.queues.iter().map(|q| q.name.clone().into()).collect();
    ModelRc::new(VecModel::from(names))
}

/// Bring a window in front of every other one (the browser too) for a
/// moment, then let it behave normally again.
macro_rules! to_front {
    ($w:expr) => {{
        let w = &$w;
        w.window().set_minimized(false);
        w.set_front(true);
        let weak = w.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(1800), move || {
            if let Some(w) = weak.upgrade() {
                w.set_front(false);
            }
        });
    }};
}
pub(crate) use to_front;

/// Open a window: the first time it is shown (again) it goes to
/// the middle of the screen and can't be maximized (only resized from its
/// edges); then it comes in front of the main window.
macro_rules! open_window {
    ($w:expr) => {{
        let was_open = $w.window().is_visible();
        let _ = $w.show();
        if !was_open {
            $crate::winfx::place($w.as_weak(), false);
        }
        $crate::to_front!($w);
    }};
}
pub(crate) use open_window;

pub(crate) fn bring_to_front(ctx: &Ctx) {
    if let Some(w) = ctx.main.upgrade() {
        // the main window keeps its maximize button (open_window! is for the others)
        let was_open = w.window().is_visible();
        let _ = w.show();
        if !was_open {
            winfx::place(w.as_weak(), true);
        }
        to_front!(w);
    }
}

// ------------------------------------------------------------ window sizes
// The size the user gave a window is kept (Options don't have it: it is
// dragged), and the window opens with it next time.

thread_local! {
    // the size each window had at the last look (saved once it stays the same)
    static SEEN_SIZE: RefCell<HashMap<String, (f32, f32)>> = RefCell::new(HashMap::new());
}

/// The size a window had when the program was used last.
pub(crate) fn saved_size(mgr: &Manager, key: &str) -> Option<(f32, f32)> {
    mgr.settings()
        .window_sizes
        .get(key)
        .copied()
        .filter(|&(w, h)| (200.0..10000.0).contains(&w) && (80.0..10000.0).contains(&h))
}

/// The size of `w` now, in the units the windows are made with.
fn size_now(w: &slint::Window) -> Option<(f32, f32)> {
    if !w.is_visible() || w.is_minimized() || w.is_maximized() {
        return None;
    }
    let s = w.size().to_logical(w.scale_factor());
    Some((s.width.round(), s.height.round()))
}

/// Called about once a second: a size that stayed the same since the last
/// look and is new is saved (not while the edge is still being dragged).
pub(crate) fn follow_size(mgr: &Arc<Manager>, key: &str, size: Option<(f32, f32)>) {
    let Some(size) = size else { return };
    let steady = SEEN_SIZE.with(|m| m.borrow_mut().insert(key.to_string(), size) == Some(size));
    if steady && mgr.settings().window_sizes.get(key) != Some(&size) {
        let mut s = mgr.settings();
        s.window_sizes.insert(key.to_string(), size);
        let _ = mgr.set_settings(s);
    }
}

/// The sizes of the main window and the progress windows, once a second.
fn follow_sizes(ui: &Ui) {
    if let Some(m) = ui.ctx.main.upgrade() {
        follow_size(&ui.ctx.mgr, "main", size_now(m.window()));
    }
    for (d, _) in ui.progress.borrow().values() {
        // the height counts only with the details shown (without them the
        // window is as small as it can be)
        if let Some((w, h)) = size_now(d.window()) {
            let h = if d.get_show_details() {
                h
            } else {
                saved_size(&ui.ctx.mgr, "progress").map(|s| s.1).unwrap_or(0.0)
            };
            follow_size(&ui.ctx.mgr, "progress", Some((w, h)));
        }
    }
}

/// Switch the whole program to another language right away.
pub(crate) fn set_language(ui: &Ui, lang: &str) {
    i18n::apply(lang);
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_language(lang.into());
    }
    // every window reads from the side of the language
    apply_look(ui, &Look::of(&ui.ctx.mgr.settings()));
    refresh(ui);
}

/// Set when the program should open again after it closes (the windows
/// change their reading direction).
static RESTART: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The language was changed by the user: when the windows now read from the
/// other side, the program opens again so everything starts that way.
pub(crate) fn change_language(ui: &Ui, lang: &str) {
    let was_rtl = i18n::is_rtl();
    set_language(ui, lang);
    if was_rtl != i18n::is_rtl() {
        // after the menu / Options window is gone
        Timer::single_shot(Duration::from_millis(200), move || {
            if ask(&t("Ezoo Download will restart now to show its windows in the direction of the new language. Downloads continue after it opens.\n\nRestart now?")) {
                RESTART.store(true, std::sync::atomic::Ordering::SeqCst);
                let _ = slint::quit_event_loop();
            }
        });
    }
}

/// Open the program again (after it saved everything and closed).
fn restart_now() {
    let Ok(exe) = std::env::current_exe() else { return };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--restarted");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS: not tied to this one
        cmd.creation_flags(0x0000_0008);
    }
    let _ = cmd.spawn();
}

/// Fonts of Options > Appearance > Font ("" = Windows' own).
pub(crate) const FONTS: [&str; 7] = ["", "Tahoma", "Arial", "Calibri", "Verdana", "Times New Roman", "Courier New"];

/// Everything that decides how the windows look.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Look {
    pub mode: i32,
    /// 0.8 ..= 1.5
    pub scale: f32,
    pub family: String,
    /// picture behind the main window ("" = none)
    pub background: String,
    /// 0.1 ..= 0.8: how much of the picture shows through
    pub strength: f32,
}

impl Look {
    pub(crate) fn of(s: &Settings) -> Look {
        Look {
            mode: s.theme.min(8) as i32,
            scale: s.font_scale.clamp(80, 150) as f32 / 100.0,
            family: if FONTS.contains(&s.font_family.as_str()) { s.font_family.clone() } else { String::new() },
            background: s.background_image.clone(),
            strength: s.background_strength.clamp(10, 80) as f32 / 100.0,
        }
    }
}

thread_local! {
    // the picture behind the main window, loaded once per file
    static BACKGROUND: RefCell<(String, Option<slint::Image>)> = const { RefCell::new((String::new(), None)) };
}

/// The picture of `path` (a png / jpg the user chose); None when it can't be read.
fn background_image(path: &str) -> Option<slint::Image> {
    if path.trim().is_empty() {
        return None;
    }
    BACKGROUND.with(|b| {
        let mut b = b.borrow_mut();
        if b.0 != path {
            // a picture is at most a few MB; anything huge is not loaded
            let ok = std::fs::metadata(path).map(|m| m.is_file() && m.len() <= 40 << 20).unwrap_or(false);
            *b = (path.to_string(), if ok { slint::Image::load_from_path(Path::new(path)).ok() } else { None });
        }
        b.1.clone()
    })
}

/// Set the theme on every open window (each window has its own copy).
pub(crate) fn apply_theme(ui: &Ui, mode: i32) {
    let mut look = Look::of(&ui.ctx.mgr.settings());
    look.mode = mode;
    apply_look(ui, &look);
}

fn set_theme_of(t: Theme<'_>, look: &Look) {
    t.set_rtl(i18n::is_rtl());
    t.set_mode(look.mode);
    t.set_font_scale(look.scale);
    t.set_font_family(look.family.clone().into());
}

/// Show `look` on every window right away.
pub(crate) fn apply_look(ui: &Ui, look: &Look) {
    macro_rules! set {
        ($($w:expr),*) => { $( if let Some(w) = $w.upgrade() { set_theme_of(w.global::<Theme>(), look); } )* };
    }
    set!(
        ui.ctx.main,
        ui.ctx.add,
        ui.batch,
        ui.settings_dlg,
        ui.complete,
        ui.columns,
        ui.category,
        ui.find,
        ui.props,
        ui.dup,
        ui.countdown,
        ui.sched,
        ui.grabber,
        ui.tips,
        ui.refresh_dlg,
        ui.playlist,
        ui.torrent,
        ui.browsers,
        ui.updater
    );
    ABOUT.with(|a| {
        if let Some(d) = a.borrow().as_ref().and_then(|w| w.upgrade()) {
            set_theme_of(d.global::<Theme>(), look);
        }
    });
    for (d, _) in ui.progress.borrow().values() {
        set_theme_of(d.global::<Theme>(), look);
    }
    // the main window only: the picture behind it
    if let Some(m) = ui.ctx.main.upgrade() {
        match background_image(&look.background) {
            Some(img) => {
                m.set_bg_image(img);
                m.set_has_bg_image(true);
                m.global::<Theme>().set_glass(look.strength);
            }
            None => {
                m.set_has_bg_image(false);
                m.global::<Theme>().set_glass(0.0);
            }
        }
    }
}

pub(crate) fn cols_from(s: &Settings) -> Cols {
    let c = |k: &str| s.column_visible(k);
    Cols {
        q: c("q"),
        size: c("size"),
        status: c("status"),
        eta: c("eta"),
        speed: c("speed"),
        last: c("last"),
        desc: c("desc"),
        added: c("added"),
        saveto: c("saveto"),
        referer: c("referer"),
        parent: c("parent"),
    }
}

/// Which column a point `x` of the header is over (the header starts with
/// 6 px of padding and the 32 px file type column).
pub(crate) fn column_at(widths: &[f32], x: f32) -> usize {
    let mut left = 6.0 + 32.0;
    for (i, w) in widths.iter().enumerate() {
        if x < left + w {
            return i;
        }
        left += w;
    }
    widths.len().saturating_sub(1)
}

/// Keep the header's column order and widths in the settings.
fn save_columns(ui: &Ui) {
    let cols: Vec<ColDef> = (0..ui.columns_model.row_count()).filter_map(|i| ui.columns_model.row_data(i)).collect();
    let mut s = ui.ctx.mgr.settings();
    let mut order: Vec<String> = cols.iter().map(|c| c.key.to_string()).collect();
    // hidden columns keep their place at the end
    let rest: Vec<String> =
        s.column_order.iter().cloned().chain(swiftget_engine::config::ALL_COLUMNS.iter().map(|k| k.to_string())).collect();
    for k in rest {
        if !order.contains(&k) {
            order.push(k);
        }
    }
    s.column_order = order;
    for c in &cols {
        s.column_widths.insert(c.key.to_string(), c.width.round().clamp(36.0, 2000.0) as u32);
    }
    let _ = ui.ctx.mgr.set_settings(s);
}

/// Look of the main window from the settings (columns, toolbar, sorting).
pub(crate) fn apply_view_settings(ui: &Ui, s: &Settings) {
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_hide_categories(s.hide_categories);
        m.set_sort_by(s.sort_by.clone().into());
        m.set_sort_desc(s.sort_desc);
        m.set_cols(cols_from(s));
        let defs: Vec<ColDef> =
            s.column_layout().into_iter().map(|(key, w)| ColDef { key: key.into(), width: w as f32 }).collect();
        ui.columns_model.set_vec(defs);
        m.set_toolbar_small(s.toolbar_small);
    }
}

fn open_batch(ui: &Ui, text: String) {
    let Some(d) = ui.batch.upgrade() else { return };
    let s = ui.ctx.mgr.settings();
    d.set_queue_names(queue_names(&s));
    let current = ui.ctx.main.upgrade().map(|m| m.get_current_queue() as u64).unwrap_or(MAIN_QUEUE);
    d.set_queue_index(s.queues.iter().position(|q| q.id == current).unwrap_or(0) as i32);
    d.set_links(extract_links(&text).join("\n").into());
    d.set_mode(0);
    d.set_folder(SharedString::new());
    d.set_start_now(true);
    d.set_message(SharedString::new());
    d.set_preview(SharedString::new());
    open_window!(d);
}

fn close_complete(ui: &Ui) {
    if let Some(d) = ui.complete.upgrade() {
        if d.get_dont_show() {
            let mut s = ui.ctx.mgr.settings();
            s.show_complete_dialog = false;
            let _ = ui.ctx.mgr.set_settings(s);
        }
        let _ = d.hide();
    }
}

/// Fill "Download File Info" with a name: category, Save As, icon.
fn show_name_in_dialog(ctx: &Ctx, d: &AddDialog, name: &str, url: &str) -> String {
    let s = ctx.mgr.settings();
    let cat = s.category_of(name, url);
    let path = s.dir_for(&cat).join(name).display().to_string();
    d.set_save_as(path.clone().into());
    let cats = ctx.add_state.lock().unwrap().categories.clone();
    d.set_category_index(cats.iter().position(|c| *c == cat).unwrap_or(0) as i32);
    d.set_category_name(t(&cat).into());
    d.set_kind(kind_for(name).into());
    d.set_ext(filename::extension(name).to_uppercase().into());
    path
}

pub(crate) fn open_add_dialog(ctx: &Ctx, url: String, headers: RequestHeaders, name_hint: Option<String>, extra: VideoExtra) {
    // a magnet link: the torrent window
    if swiftget_engine::torrent::is_magnet(&url) {
        torrent_ui::open_source(&url);
        return;
    }
    let Some(d) = ctx.add.upgrade() else { return };
    let s = ctx.mgr.settings();
    if d.window().is_visible() {
        let mut st = ctx.add_state.lock().unwrap();
        // the same link again (a page can send it twice): nothing new to show;
        // another link waits until this one is started or cancelled
        if !url.is_empty() && st.origin_url != url && !st.waiting.iter().any(|p| p.url == url) {
            st.waiting.push_back(PendingOpen { url, headers, name_hint, extra });
        }
        drop(st);
        to_front!(d);
        return;
    }
    // nothing may be left over from an earlier window
    cancel_early(ctx);
    // General first, like the classic list
    let mut cats = vec!["General".to_string()];
    cats.extend(s.categories.iter().map(|c| c.name.clone()));
    let names: Vec<SharedString> = cats.iter().map(|c| t(c).into()).collect();
    d.set_category_names(ModelRc::new(VecModel::from(names)));
    ctx.add_state.lock().unwrap().categories = cats;
    let name = name_hint
        .clone()
        .filter(|n| !n.trim().is_empty())
        .map(|n| filename::sanitize(&n))
        .or_else(|| filename::from_url(&url).map(|n| filename::sanitize(&n)))
        .unwrap_or_else(|| "download".into());
    let path = show_name_in_dialog(ctx, &d, &name, &url);
    let login = headers.auth.clone().or_else(|| s.login_for(&url));
    {
        let mut st = ctx.add_state.lock().unwrap();
        st.headers = headers;
        st.extra = extra;
        st.origin_url = url.clone();
        st.probe = None;
        st.probe_url.clear();
        // a name from the browser is final; one guessed from the URL is not
        st.auto_path = if name_hint.is_some() { String::new() } else { path };
        st.probe_seq += 1;
    }
    d.set_url(url.clone().into());
    d.set_description(SharedString::new());
    d.set_remember_path(false);
    d.set_size_text(SharedString::new());
    d.set_connections(s.connections() as i32);
    d.set_queue_names(queue_names(&s));
    d.set_queue_index(0);
    d.set_use_login(login.is_some());
    d.set_user(login.as_ref().map(|l| l.0.clone()).unwrap_or_default().into());
    d.set_password(login.map(|l| l.1).unwrap_or_default().into());
    d.set_warning(
        if !url.is_empty() && ctx.mgr.find_url(&url).is_some() {
            t("This address is already in the download list.")
        } else {
            String::new()
        }
        .into(),
    );
    d.set_info(t("Paste a link and press Check").into());
    d.set_can_use_browser(false);
    d.set_info_error(false);
    d.set_probing(false);
    open_window!(d);
    // it already downloads while the window is open
    if !url.is_empty() && s.start_while_info && ctx.mgr.find_url(&url).is_none() {
        let path = PathBuf::from(d.get_save_as().as_str());
        let st = ctx.add_state.lock().unwrap();
        let req = NewDownload {
            url: url.clone(),
            file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            save_dir: path.parent().map(Path::to_path_buf),
            headers: st.headers.clone(),
            audio_url: st.extra.audio_url.clone(),
            source_page: st.extra.source_page.clone(),
            source_key: st.extra.source_key.clone(),
            early: true,
            start: true,
            ..Default::default()
        };
        drop(st);
        let id = ctx.mgr.add(req);
        ctx.add_state.lock().unwrap().early_id = Some(id);
    }
    if !url.is_empty() {
        start_probe(ctx, url);
    }
}

/// "Cancel" in Download File Info: the download that started meanwhile goes
/// away completely, with its temporary files and the file itself.
fn cancel_early(ctx: &Ctx) {
    if let Some(id) = ctx.add_state.lock().unwrap().early_id.take() {
        ctx.mgr.discard(id);
    }
}

/// The window is free again: show the next link that came in meanwhile.
fn open_next(ctx: &Ctx) {
    let next = ctx.add_state.lock().unwrap().waiting.pop_front();
    if let Some(p) = next {
        let ctx = ctx.clone();
        Timer::single_shot(Duration::from_millis(300), move || {
            open_add_dialog(&ctx, p.url, p.headers, p.name_hint, p.extra);
        });
    }
}

/// The download started in the background for the open window, if any.
pub(crate) fn early_download(ctx: &Ctx) -> Option<u64> {
    ctx.add_state.lock().unwrap().early_id
}

/// Ask the server about the file in the background, then show the result.
fn start_probe(ctx: &Ctx, url: String) {
    let Some(d) = ctx.add.upgrade() else { return };
    let (seq, mut headers) = {
        let mut st = ctx.add_state.lock().unwrap();
        st.probe_seq += 1;
        st.probe = None;
        (st.probe_seq, st.headers.clone())
    };
    if d.get_use_login() && !d.get_user().trim().is_empty() {
        headers.auth = Some((d.get_user().trim().to_string(), d.get_password().to_string()));
    }
    d.set_probing(true);
    d.set_info_error(false);
    let ctx2 = ctx.clone();
    ctx.rt.spawn(async move {
        let result = ctx2.mgr.probe(&url, &headers).await;
        let _ = slint::invoke_from_event_loop(move || {
            let Some(d) = ctx2.add.upgrade() else { return };
            let mut st = ctx2.add_state.lock().unwrap();
            if st.probe_seq != seq {
                return; // the window was reused for something else meanwhile
            }
            d.set_probing(false);
            match result {
                Ok(p) => {
                    if d.get_save_as().as_str() == st.auto_path && !st.auto_path.is_empty() {
                        drop(st);
                        let path = show_name_in_dialog(&ctx2, &d, &p.file_name, &url);
                        st = ctx2.add_state.lock().unwrap();
                        st.auto_path = path;
                    }
                    let has_audio = st.extra.audio_url.is_some() && st.origin_url == url;
                    let size = p.total_size.map(fmt_size).unwrap_or_else(|| t("Unknown size"));
                    let size_text = if has_audio { format!("{size}\n{}", t("+ audio")) } else { size };
                    d.set_size_text(size_text.into());
                    let kind = match (p.kind, p.resumable) {
                        _ if has_audio => "Video + audio, joined after download",
                        (DownloadKind::Hls, _) => "Video stream (HLS)",
                        (_, true) => "Resume supported",
                        (_, false) => "No resume support",
                    };
                    d.set_can_use_browser(false);
                    d.set_info(format!("{}  ·  {}", t(kind), p.content_type.clone().unwrap_or_default()).into());
                    d.set_info_error(false);
                    if p.is_web_page() {
                        d.set_warning(
                            t("This address opens a web page, not a file. The link may have expired or need a login.")
                                .into(),
                        );
                    }
                    st.probe_url = url;
                    st.probe = Some(p);
                }
                Err(e) => {
                    let text = format!("{e:#}");
                    // refused (often a site that only serves its own browser
                    // session): the browser can still take it
                    d.set_can_use_browser(["HTTP 403", "HTTP 406", "HTTP 429"].iter().any(|c| text.contains(c)));
                    d.set_info(text.into());
                    d.set_info_error(true);
                }
            }
        });
    });
}

/// Add the download from the "Download File Info" window.
fn confirm_add(ui: &Ui, start: bool) {
    let ctx = &ui.ctx;
    let Some(d) = ctx.add.upgrade() else { return };
    let url = d.get_url().trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) || url::Url::parse(&url).is_err() {
        d.set_info(t("Please enter a valid http:// or https:// address").into());
        d.set_info_error(true);
        return;
    }
    let save_as = PathBuf::from(d.get_save_as().trim());
    // the name as the program suggested it (its extension is the real one)
    let suggested = {
        let st = ctx.add_state.lock().unwrap();
        let early = st.early_id.and_then(|id| ctx.mgr.get(id)).map(|v| v.file_name);
        let auto = Path::new(&st.auto_path).file_name().map(|n| n.to_string_lossy().into_owned());
        early.or(auto).or_else(|| st.probe.as_ref().map(|p| p.file_name.clone())).unwrap_or_default()
    };
    let name = save_as
        .file_name()
        .map(|n| with_extension_of(&n.to_string_lossy(), &suggested))
        .filter(|n| !n.trim().is_empty());
    let dir = save_as.parent().map(Path::to_path_buf).filter(|p| !p.as_os_str().is_empty());
    let settings = ctx.mgr.settings();
    if d.get_remember_path() {
        if let Some(dir) = &dir {
            let mut s = settings.clone();
            let key = ctx
                .add_state
                .lock()
                .unwrap()
                .categories
                .get(d.get_category_index().max(0) as usize)
                .cloned()
                .unwrap_or_else(|| "General".into());
            s.category_dirs.insert(key, dir.clone());
            let _ = ctx.mgr.set_settings(s);
        }
    }
    let req = {
        let mut st = ctx.add_state.lock().unwrap();
        st.probe_seq += 1;
        let probe = if st.probe_url == url { st.probe.clone() } else { None };
        // the video extras belong to the original link only
        let extra = if st.origin_url == url { st.extra.clone() } else { VideoExtra::default() };
        // "Download Later" puts it in the chosen queue
        let queue = if start {
            None
        } else {
            settings.queues.get(d.get_queue_index().max(0) as usize).map(|q| q.id)
        };
        let description = d.get_description().trim().to_string();
        let mut headers = st.headers.clone();
        headers.auth = (d.get_use_login() && !d.get_user().trim().is_empty())
            .then(|| (d.get_user().trim().to_string(), d.get_password().to_string()));
        NewDownload {
            url: url.clone(),
            file_name: name,
            save_dir: dir,
            headers,
            connections: Some(d.get_connections().max(1) as u32),
            probe,
            audio_url: extra.audio_url,
            source_page: extra.source_page,
            source_key: extra.source_key,
            queue,
            description: (!description.is_empty()).then_some(description),
            pick_quality: None,
            early: false,
            start,
            torrent_select: None,
        }
    };
    let _ = d.hide();
    open_next(ctx);
    let early = ctx.add_state.lock().unwrap().early_id.take();
    if let Some(id) = early {
        if ctx.mgr.get(id).map(|v| v.url == url || v.parent_page.as_deref() == Some(url.as_str())).unwrap_or(false)
            || ctx.add_state.lock().unwrap().origin_url == url
        {
            let dir = req.save_dir.clone().unwrap_or_else(|| settings.dir_for("General"));
            let name = req.file_name.clone().unwrap_or_default();
            if let Err(e) = ctx.mgr.retarget(id, &dir, &name, req.description.clone(), req.headers.auth.clone()) {
                message(&tf("Could not change where it is saved: {}", &[&format!("{e:#}")]));
            }
            if start {
                if ctx.mgr.get(id).map(|v| v.status == Status::Failed || v.status == Status::Paused).unwrap_or(false) {
                    ctx.mgr.start(id);
                }
                // finished while the window was open: "Download complete"
                // now; still running: its progress window
                if ctx.mgr.get(id).map(|v| v.status == Status::Completed).unwrap_or(false) {
                    if settings.sounds_on {
                        sound::play(&settings.sound_complete);
                    }
                    if settings.show_complete_dialog {
                        show_complete(ui, id);
                    }
                } else {
                    show_progress_for_start(ui, id);
                }
            } else {
                // Download Later: stop it and put it in the queue
                ctx.mgr.pause(id);
                if let Some(q) = req.queue {
                    ctx.mgr.set_queue(id, Some(q));
                }
            }
            refresh(ui);
            return;
        }
        ctx.mgr.remove(id, false);
    }
    match ctx.mgr.find_url(&url) {
        Some(existing) => {
            let choice = settings.duplicate_action;
            if choice == 0 {
                extra::ask_duplicate(ui, PendingAdd { req, existing });
            } else {
                apply_duplicate(ui, PendingAdd { req, existing }, choice);
            }
        }
        None => {
            finish_add(ui, req);
        }
    }
}

/// A name typed without its extension ("My video") gets the one of the
/// file (".mp4"), so Windows knows how to open it.
pub(crate) fn with_extension_of(name: &str, suggested: &str) -> String {
    let name = name.trim();
    let ext = Path::new(suggested).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let has_ext = Path::new(name)
        .extension()
        .map(|e| !e.is_empty() && e.len() <= 5 && e.to_string_lossy().chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or(false);
    if has_ext || ext.is_empty() || name.is_empty() {
        name.to_string()
    } else {
        format!("{name}.{ext}")
    }
}

/// Really add it; open its progress window when it starts.
pub(crate) fn finish_add(ui: &Ui, req: NewDownload) -> u64 {
    let start = req.start;
    let id = ui.ctx.mgr.add(req);
    if start {
        show_progress_for_start(ui, id);
    }
    refresh(ui);
    id
}

/// What to do with a link that is already in the list: 1 add numbered,
/// 2 replace the old one, 3 show / resume the old one.
pub(crate) fn apply_duplicate(ui: &Ui, p: PendingAdd, choice: u32) {
    match choice {
        2 => {
            ui.ctx.mgr.remove(p.existing, true);
            // the old file goes away a moment later; wait for it
            let ui2 = ui.clone();
            Timer::single_shot(Duration::from_millis(1500), move || {
                finish_add(&ui2, p.req);
            });
        }
        3 => match ui.ctx.mgr.get(p.existing) {
            Some(v) if v.status == Status::Completed => show_complete(ui, v.id),
            Some(v) => {
                ui.ctx.mgr.start(v.id);
                open_progress(ui, v.id);
            }
            None => {
                finish_add(ui, p.req);
            }
        },
        _ => {
            finish_add(ui, p.req);
        }
    }
}

/// Progress window for a download that was just started (Options > Downloads).
pub(crate) fn show_progress_for_start(ui: &Ui, id: u64) {
    match ui.ctx.mgr.settings().progress_mode {
        0 => open_progress(ui, id),
        1 => {
            open_progress(ui, id);
            if let Some((d, _)) = ui.progress.borrow().get(&id) {
                d.window().set_minimized(true);
            }
        }
        _ => {}
    }
}

fn row_action(ui: &Ui, id: u64, action: &str) {
    let Some(v) = ui.ctx.mgr.get(id) else { return };
    match action {
        "open" => {
            open_download(&v.save_path);
        }
        "folder" => reveal_in_folder(&v),
        "openwith" => open_with(&v.save_path),
        "preview" => play_while_downloading(ui, id),
        "resume" => {
            ui.ctx.mgr.start(id);
            show_progress_for_start(ui, id);
        }
        "stop" => ui.ctx.mgr.pause(id),
        "restart" => {
            if ask(&tf("Download “{}” again from the beginning?", &[&v.file_name])) {
                ui.ctx.mgr.restart(id);
            }
        }
        "refresh" => extra::open_refresh(ui, id),
        "copy" => {
            if let Ok(mut c) = arboard::Clipboard::new() {
                let _ = c.set_text(v.url.clone());
            }
        }
        "move" => {
            if v.status == Status::Completed {
                let mut dlg = crate::winfx::file_box().set_file_name(&v.file_name);
                if let Some(dir) = v.save_path.parent() {
                    dlg = dlg.set_directory(dir);
                }
                if let Some(to) = dlg.save_file() {
                    let to = {
                        let n = to.file_name().map(|n| with_extension_of(&n.to_string_lossy(), &v.file_name)).unwrap_or_default();
                        to.with_file_name(n)
                    };
                    // on its own thread: a move to another disk is a copy
                    let mgr = ui.ctx.mgr.clone();
                    std::thread::spawn(move || {
                        // the save window asked already before a file was replaced
                        if let Err(e) = mgr.move_file(id, &to, true) {
                            let text = tf("Could not move the file:\n\n{}", &[&format!("{e:#}")]);
                            let _ = slint::invoke_from_event_loop(move || message(&text));
                        }
                    });
                }
            } else {
                extra::open_properties(ui, id);
            }
        }
        "sha256" => {
            let path = v.save_path.clone();
            let name = v.file_name.clone();
            std::thread::spawn(move || {
                let text = match sha256_of(&path) {
                    Ok(hash) => {
                        let shown = tf("SHA-256 of {}:\n\n{}\n\n(copied to the clipboard)", &[&name, &hash]);
                        if let Ok(mut c) = arboard::Clipboard::new() {
                            let _ = c.set_text(hash);
                        }
                        shown
                    }
                    Err(e) => tf("Could not read the file: {}", &[&e]),
                };
                let _ = slint::invoke_from_event_loop(move || message(&text));
            });
        }
        "properties" => extra::open_properties(ui, id),
        "delete" => delete_download(&ui.ctx, id),
        _ => {}
    }
}

fn sha256_of(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// "Open with…": Windows' own list of programs for the file. On a thread
/// of its own: the windows never wait for it.
pub(crate) fn open_with(path: &Path) {
    let path = path.to_path_buf();
    #[cfg(windows)]
    std::thread::spawn(move || {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::{SHOpenWithDialog, OAIF_ALLOW_REGISTRATION, OAIF_EXEC, OPENASINFO};
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let info = OPENASINFO {
            pcszFile: wide.as_ptr(),
            pcszClass: std::ptr::null(),
            oaifInFlags: OAIF_ALLOW_REGISTRATION | OAIF_EXEC,
        };
        // SAFETY: `wide` is a NUL-terminated path that outlives the call
        let hr = unsafe { SHOpenWithDialog(std::ptr::null_mut(), &info) };
        if hr < 0 {
            // an older Windows: the classic way
            let _ = std::process::Command::new(swiftget_engine::system::windows_tool("rundll32.exe"))
                .arg("shell32.dll,OpenAs_RunDLL")
                .arg(&path)
                .spawn();
        }
    });
    #[cfg(not(windows))]
    {
        let _ = open::that_detached(path);
    }
}

/// Delete one download, or several with one question.
fn delete_downloads(ui: &Ui, ids: Vec<u64>) {
    match ids.len() {
        0 => {}
        1 => delete_download(&ui.ctx, ids[0]),
        n => {
            let ui2 = ui.clone();
            let text = tf("Remove these {} downloads from the download list?", &[&n]);
            ask_remove(&ui.ctx, &t("Delete downloads"), &text, move |delete_files| {
                if delete_files {
                    for id in &ids {
                        ui2.ctx.mgr.remove(*id, true);
                    }
                } else {
                    // the list only (parts of unfinished downloads are cleaned up)
                    ui2.ctx.mgr.remove_many(&ids);
                }
                ui2.sel.borrow_mut().ids.clear();
                if let Some(w) = ui2.ctx.main.upgrade() {
                    w.set_selected_id(-1);
                }
                refresh(&ui2);
            });
        }
    }
}

fn delete_download(ctx: &Ctx, id: u64) {
    let Some(v) = ctx.mgr.get(id) else { return };
    let text = if v.status == Status::Completed {
        tf("Remove “{}” from the download list?", &[&v.file_name])
    } else {
        tf("Delete the unfinished download “{}”?\nThe partly downloaded data will be removed.", &[&v.file_name])
    };
    let ctx2 = ctx.clone();
    ask_remove(ctx, &t("Delete download"), &text, move |delete_files| {
        // its progress window goes away with it
        ctx2.mgr.remove(id, delete_files);
        if let Some(w) = ctx2.main.upgrade() {
            if w.get_selected_id() == id as i32 {
                w.set_selected_id(-1);
            }
        }
    });
}

/// Removing downloads from the list: the question (with "delete the files
/// from the disk too" and "don't show this again"), or straight away when
/// the user said not to ask. `remove` gets whether files go too.
fn ask_remove(ctx: &Ctx, title: &str, text: &str, remove: impl FnOnce(bool) + 'static) {
    let s = ctx.mgr.settings();
    if !s.ask_before_remove {
        // without the question only the list entries go: files are never
        // deleted from the disk without the user seeing it right then
        remove(false);
        return;
    }
    let mgr = ctx.mgr.clone();
    confirm_with(ctx, title, text, Some(s.remove_deletes_files), move |delete_files, dont_ask| {
        let mut s = mgr.settings();
        if s.remove_deletes_files != delete_files || s.ask_before_remove == dont_ask {
            s.remove_deletes_files = delete_files;
            s.ask_before_remove = !dont_ask;
            let _ = mgr.set_settings(s);
        }
        remove(delete_files);
    });
}

thread_local! {
    // the question windows that are open now
    static CONFIRMS: RefCell<Vec<ConfirmDialog>> = const { RefCell::new(Vec::new()) };
}

/// What runs when a question is answered with OK.
type OnOk = Box<dyn FnOnce(bool, bool)>;

/// A yes / no question in a window of our own: it is shown in front of the
/// download windows and the program goes on working while it waits (a
/// Windows message box could end up behind them and seem to freeze it all).
/// `on_ok` runs when OK is pressed.
pub(crate) fn confirm(ctx: &Ctx, title: &str, text: &str, on_ok: impl FnOnce() + 'static) {
    confirm_with(ctx, title, text, None, move |_, _| on_ok());
}

/// The same; `delete_files`: a question about removing downloads, with its
/// two check boxes (this one ticked as given). `on_ok` gets both of them.
fn confirm_with(ctx: &Ctx, title: &str, text: &str, delete_files: Option<bool>, on_ok: impl FnOnce(bool, bool) + 'static) {
    let Ok(d) = ConfirmDialog::new() else {
        if ask(text) {
            on_ok(false, false);
        }
        return;
    };
    set_theme_of(d.global::<Theme>(), &Look::of(&ctx.mgr.settings()));
    d.set_heading(title.into());
    d.set_text(text.into());
    d.set_removing(delete_files.is_some());
    d.set_delete_files(delete_files.unwrap_or(false));
    let pending: Rc<RefCell<Option<OnOk>>> = Rc::new(RefCell::new(Some(Box::new(on_ok))));
    let close = |d: &ConfirmDialog| {
        let _ = d.hide();
        // dropped after this event is over
        Timer::single_shot(Duration::from_millis(1), || {
            CONFIRMS.with(|c| c.borrow_mut().retain(|x| x.window().is_visible()));
        });
    };
    {
        let (w, pending) = (d.as_weak(), pending.clone());
        d.on_answered(move |ok| {
            let Some(d) = w.upgrade() else { return };
            close(&d);
            let f = pending.borrow_mut().take();
            if ok {
                if let Some(f) = f {
                    f(d.get_delete_files(), d.get_dont_ask());
                }
            }
        });
    }
    {
        let w = d.as_weak();
        d.window().on_close_requested(move || {
            if let Some(d) = w.upgrade() {
                close(&d);
            }
            slint::CloseRequestResponse::HideWindow
        });
    }
    open_window!(d);
    CONFIRMS.with(|c| c.borrow_mut().push(d));
}

pub(crate) fn reveal_in_folder(v: &DownloadView) {
    let dir = v.save_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if v.status == Status::Completed && v.save_path.exists() {
            let _ = std::process::Command::new(swiftget_engine::system::windows_tool("explorer.exe"))
                .raw_arg(format!("/select,\"{}\"", v.save_path.display()))
                .spawn();
            return;
        }
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = open::that_detached(&dir);
}

pub(crate) fn state_of(s: Status) -> i32 {
    match s {
        Status::Queued => 0,
        Status::Connecting | Status::Downloading | Status::Assembling => 1,
        Status::Paused => 2,
        Status::Completed => 3,
        Status::Failed => 4,
    }
}

/// Order of the progress window's "when it completes" list:
/// nothing, shut down, sleep, exit, hibernate.
fn completion_index(a: AfterDone) -> i32 {
    match a {
        AfterDone::Nothing => 0,
        AfterDone::Shutdown => 1,
        AfterDone::Sleep => 2,
        AfterDone::Exit => 3,
        AfterDone::Hibernate => 4,
    }
}

fn completion_from(i: i32) -> AfterDone {
    match i {
        1 => AfterDone::Shutdown,
        2 => AfterDone::Sleep,
        3 => AfterDone::Exit,
        4 => AfterDone::Hibernate,
        _ => AfterDone::Nothing,
    }
}

/// Downloads > When all downloads finish (same numbers as the progress window).
fn menu_done_from(i: i32) -> AfterDone {
    completion_from(i)
}

pub(crate) fn fmt_time(t: chrono::DateTime<chrono::Utc>) -> String {
    t.with_timezone(&chrono::Local).format("%b %d %H:%M").to_string()
}

/// Open what is already downloaded of a video / song in the player.
pub(crate) fn play_while_downloading(ui: &Ui, id: u64) {
    let Some(v) = ui.ctx.mgr.get(id) else { return };
    let kind = kind_for(&v.file_name);
    if kind != "video" && kind != "audio" {
        return message(&t("Only videos and sound files can be played while they download."));
    }
    match ui.ctx.mgr.preview_source(id) {
        None if v.kind == DownloadKind::Hls => {
            message(&t("This video comes in many small pieces that are joined at the end: it can be played when the download is done."))
        }
        None => message(&t("Nothing to play yet: wait until a part of the file is downloaded, then try again.")),
        Some(swiftget_engine::PreviewSource::File { path, .. }) => {
            if let Err(e) = preview::play(&path, &v.file_name) {
                message(&tf("Could not open the player: {}", &[&e]));
            }
        }
        Some(swiftget_engine::PreviewSource::Pair { video, video_ready, audio, audio_ready }) => {
            preview::play_pair(video, video_ready, audio.map(|a| (a, audio_ready)), &v.file_name, |err| {
                if let Some(e) = err {
                    let _ = slint::invoke_from_event_loop(move || message(&tf("Could not open the player: {}", &[&e])));
                }
            });
        }
    }
}

/// Open a finished download. A .torrent file opens in our own torrent
/// window (what it shares is the real download).
pub(crate) fn open_download(path: &std::path::Path) {
    if path.extension().map(|e| e.eq_ignore_ascii_case("torrent")).unwrap_or(false) && path.is_file() {
        torrent_ui::open_source(&path.display().to_string());
    } else {
        // the program that opens it may take its time (or Windows asks which
        // one to use): the windows don't wait for it
        let _ = open::that_detached(path);
    }
}

/// An error of the engine in words the user understands, in their language.
pub(crate) fn err_text(e: &str) -> String {
    if e.contains("needs a login or an age check") {
        t("This video needs a login or an age check. Open it once in the browser where you are signed in (with the Ezoo Download extension), then try again.")
    } else if e.contains("BitLocker") || e.contains("-2144272384") {
        t("The drive of the download folder is locked by BitLocker. Unlock it in Windows, or choose another folder (right-click the download > Properties).")
    } else if e.contains("is already registered") {
        t("This torrent is already running (it may still be sharing its files). Wait a little, or exit Ezoo Download and open it again.")
    } else if let Some(rest) = e.split("the link stopped working - getting a new one in ").nth(1) {
        let secs = rest.split(' ').next().unwrap_or("?");
        tf("The link stopped working. A new one is fetched in {} s and checked before the download goes on.", &[&secs])
    } else if e.contains("ffmpeg.exe is missing") {
        t("ffmpeg.exe (it joins the video and the sound) is missing and could not be downloaded again. Allow it in your antivirus or install Ezoo Download again, then click Resume: nothing downloaded is lost.")
    } else if e.contains("the new link is not the same file") {
        t("The new link gives another file, so nothing more was written. Right-click the download and use \"Refresh download address\".")
    } else if e.contains("the link has expired or the file was removed") {
        t("The link has expired or the file was removed. Right-click the download and use \"Refresh download address\".")
    } else {
        e.to_string()
    }
}

fn row_of(v: &DownloadView, sel: bool) -> DownloadRow {
    let pct = v.progress();
    let size = match v.total_size {
        Some(t) => fmt_size(t),
        None if v.downloaded > 0 => fmt_size(v.downloaded),
        None => String::new(),
    };
    let status = match v.status {
        _ if v.waiting => t("Waiting for connection…"),
        Status::Downloading if v.kind == DownloadKind::Torrent && v.total_size.is_none() => t("Getting the file list…"),
        Status::Downloading | Status::Paused if v.total_size.is_some() => format!("{:.2}%", pct * 100.0),
        Status::Downloading if v.parts_total > 0 => tf("{}/{} parts", &[&v.parts_done, &v.parts_total]),
        Status::Downloading => fmt_size(v.downloaded),
        Status::Paused => String::new(),
        Status::Assembling => t("Merging…"),
        Status::Failed => t("Error"),
        other => t(other.label()),
    };
    let speed = if v.status.is_running() && v.speed > 0.0 {
        format!("{}/s", fmt_size(v.speed as u64))
    } else {
        String::new()
    };
    let description = match (&v.error, &v.description, v.kind) {
        (Some(e), _, _) if v.status == Status::Failed => err_text(e),
        (_, Some(d), _) => d.clone(),
        (_, _, DownloadKind::Dash) => tf("{}: video + audio", &[&t(&v.category)]),
        (_, _, DownloadKind::Hls) => tf("{}: stream", &[&t(&v.category)]),
        (_, _, DownloadKind::Torrent) if v.status.is_running() => tf("Torrent: {} peers", &[&v.active_connections]),
        (_, _, DownloadKind::Torrent) => t("Torrent"),
        _ => t(&v.category),
    };
    DownloadRow {
        id: v.id as i32,
        name: v.file_name.clone().into(),
        kind: if v.kind == DownloadKind::Torrent { "torrent" } else { kind_for(&v.file_name) }.into(),
        category: v.category.clone().into(),
        queue: v.queue.map(|q| q.to_string()).unwrap_or_default().into(),
        size: size.into(),
        status: status.into(),
        progress: pct,
        eta: v.eta_secs.map(fmt_duration).unwrap_or_default().into(),
        speed: speed.into(),
        last_try: fmt_time(v.finished_at.or(v.last_try).unwrap_or(v.added_at)).into(),
        description: description.into(),
        state: state_of(v.status),
        added: fmt_time(v.added_at).into(),
        sel,
        save_to: v.save_path.parent().map(|p| p.display().to_string()).unwrap_or_default().into(),
        referer: v.referer().to_string().into(),
        parent: v.parent_page.clone().unwrap_or_default().into(),
    }
}

/// Does a download belong to the sidebar item that is selected?
fn in_view(v: &DownloadView, view: &str) -> bool {
    match view {
        "all" => true,
        "unfinished" => v.status != Status::Completed,
        "finished" => v.status == Status::Completed,
        other => {
            if let Some(c) = other.strip_prefix("cat:") {
                v.category == c
            } else if let Some(c) = other.strip_prefix("unf:") {
                v.category == c && v.status != Status::Completed
            } else if let Some(c) = other.strip_prefix("fin:") {
                v.category == c && v.status == Status::Completed
            } else if let Some(q) = other.strip_prefix("queue:") {
                v.queue.map(|x| x.to_string()).as_deref() == Some(q)
            } else {
                true
            }
        }
    }
}

fn update_model<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    if model.row_count() == rows.len() {
        for (i, r) in rows.into_iter().enumerate() {
            if model.row_data(i).as_ref() != Some(&r) {
                model.set_row_data(i, r);
            }
        }
    } else {
        model.set_vec(rows);
    }
}

pub(crate) fn refresh(ui: &Ui) {
    let Some(w) = ui.ctx.main.upgrade() else { return };
    let mut views = ui.ctx.mgr.snapshot();
    // not in the list before Start is pressed in "Download File Info"
    if let Some(early) = early_download(&ui.ctx) {
        views.retain(|v| v.id != early);
    }
    let settings = ui.ctx.mgr.settings();
    let view = w.get_view().to_string();
    let search = w.get_search().to_lowercase();

    // the queue the toolbar's Start/Stop Queue work on
    let current_queue = view
        .strip_prefix("queue:")
        .and_then(|q| q.parse::<u64>().ok())
        .filter(|q| settings.queue(*q).is_some())
        .unwrap_or(MAIN_QUEUE);
    w.set_current_queue(current_queue as i32);

    // sorting (header click / View > Arrange files)
    let order = |v: &DownloadView| state_of(v.status);
    match settings.sort_by.as_str() {
        "name" => views.sort_by_key(|v| v.file_name.to_lowercase()),
        "size" => views.sort_by_key(|v| v.total_size.unwrap_or(v.downloaded)),
        "status" => views.sort_by_key(|v| (order(v), v.id)),
        "eta" => views.sort_by_key(|v| (v.eta_secs.unwrap_or(u64::MAX), v.id)),
        "speed" => views.sort_by_key(|v| (v.speed as u64, v.id)),
        "desc" => views.sort_by_key(|v| (v.description.clone().unwrap_or_default().to_lowercase(), v.id)),
        "saveto" => views.sort_by_key(|v| (v.save_path.parent().map(|p| p.to_path_buf()), v.id)),
        "referer" => views.sort_by_key(|v| (v.referer().to_lowercase(), v.id)),
        "parent" => views.sort_by_key(|v| (v.parent_page.clone().unwrap_or_default().to_lowercase(), v.id)),
        "added" => views.sort_by_key(|v| (v.added_at, v.id)),
        _ => views.sort_by_key(|v| (v.finished_at.or(v.last_try).unwrap_or(v.added_at), v.id)),
    }
    if settings.sort_desc {
        views.reverse();
    }

    let new_rows: Vec<DownloadRow> = views
        .iter()
        .filter(|v| in_view(v, &view))
        .filter(|v| {
            search.is_empty()
                || v.file_name.to_lowercase().contains(&search)
                || v.url.to_lowercase().contains(&search)
                || v.description.as_deref().unwrap_or("").to_lowercase().contains(&search)
        })
        .map(|v| row_of(v, false))
        .collect();
    // the selection: drop rows that are gone, mark the rest
    let mut new_rows = new_rows;
    {
        let mut sel = ui.sel.borrow_mut();
        sel.ids.retain(|id| views.iter().any(|v| v.id == *id));
        let focus = w.get_selected_id();
        if sel.ids.is_empty() && focus >= 0 && views.iter().any(|v| v.id as i32 == focus) {
            sel.ids.push(focus as u64);
        }
        for r in new_rows.iter_mut() {
            r.sel = sel.ids.contains(&(r.id as u64));
        }
        let chosen: Vec<&DownloadView> = views.iter().filter(|v| sel.ids.contains(&v.id)).collect();
        w.set_sel_count(chosen.len() as i32);
        w.set_can_resume(chosen.iter().any(|v| matches!(v.status, Status::Paused | Status::Failed)));
        w.set_can_stop(chosen.iter().any(|v| v.status == Status::Queued || v.status.is_running()));
    }
    update_model(&ui.rows, new_rows);

    // sidebar: categories (General last) with their counts
    let count = |f: &dyn Fn(&DownloadView) -> bool| {
        let n = views.iter().filter(|v| f(v)).count();
        if n > 0 {
            SharedString::from(n.to_string())
        } else {
            SharedString::new()
        }
    };
    let builtin: Vec<String> = settings.categories.iter().filter(|c| c.builtin).map(|c| c.name.clone()).collect();
    let cat_rows: Vec<CatRow> = settings
        .category_names()
        .into_iter()
        .map(|name| CatRow {
            label: t(&name).into(),
            kind: kind_of_category(&name).into(),
            count: count(&|v| v.category == name),
            unfinished: count(&|v| v.category == name && v.status != Status::Completed),
            finished: count(&|v| v.category == name && v.status == Status::Completed),
            builtin: name == "General" || builtin.contains(&name),
            name: name.into(),
        })
        .collect();
    update_model(&ui.cats, cat_rows);
    w.set_count_all(count(&|_| true));
    w.set_count_unfinished(count(&|v| v.status != Status::Completed));
    w.set_count_finished(count(&|v| v.status == Status::Completed));

    let queue_rows: Vec<QueueRow> = settings
        .queues
        .iter()
        .map(|q| QueueRow {
            id: q.id as i32,
            name: q.name.clone().into(),
            count: views
                .iter()
                .filter(|v| v.queue == Some(q.id) && v.status != Status::Completed)
                .count() as i32,
            running: views.iter().any(|v| v.queue == Some(q.id) && v.status.is_running()),
        })
        .collect();
    update_model(&ui.queues, queue_rows);

    let sel = w.get_selected_id();
    let sel_state = views
        .iter()
        .find(|v| v.id as i32 == sel)
        .map(|v| state_of(v.status))
        .unwrap_or(-1);
    if sel_state == -1 && sel >= 0 {
        w.set_selected_id(-1);
        ui.sel.borrow_mut().ids.clear();
    }
    w.set_selected_state(sel_state);

    let running = views.iter().filter(|v| v.status.is_running()).count();
    let speed: f64 = views.iter().map(|v| v.speed).sum();
    let text = if running > 0 {
        tf("{} downloading  ·  {}/s", &[&running, &fmt_size(speed as u64)])
    } else {
        tf("Ready  ·  {} files in the list", &[&views.len()])
    };
    w.set_status_text(text.into());
    w.set_limiter_on(settings.speed_limit_on);
    let limiter_text = if settings.speed_limit_on {
        tf("Speed limiter: on ({} KB/s)", &[&settings.speed_limit_kb])
    } else {
        t("Speed limiter: off")
    };
    w.set_limiter_text(limiter_text.into());
    w.set_all_done(completion_index(ui.ctx.mgr.when_all_done()));
}

// ---------------------------------------------------------------- progress windows

pub(crate) fn open_progress(ui: &Ui, id: u64) {
    let Some(v) = ui.ctx.mgr.get(id) else { return };
    if let Some((d, segs)) = ui.progress.borrow().get(&id) {
        // back from next to the clock
        tray::remove_download(id);
        fill_details(d, segs, &v);
        open_window!(d);
        d.window().set_minimized(false);
        return;
    }
    let Ok(d) = DetailsDialog::new() else { return };
    let settings = ui.ctx.mgr.settings();
    set_theme_of(d.global::<Theme>(), &Look::of(&settings));
    // the details (connections) only when the user chose to see them; the
    // window opens as wide / tall as it was last time
    d.set_show_details(settings.progress_details);
    {
        let saved = saved_size(&ui.ctx.mgr, "progress");
        let width = saved.map(|s| s.0).unwrap_or(500.0).max(440.0);
        let small = d.get_content_height();
        let height = if settings.progress_details { saved.map(|s| s.1).unwrap_or(0.0).max(small) } else { small };
        d.window().set_size(slint::LogicalSize::new(width, height.max(120.0)));
    }
    // the tabs' controls are set once, so typing in them isn't overwritten
    d.set_limit_on(v.speed_limit_kb.is_some());
    d.set_limit_kb(v.speed_limit_kb.unwrap_or(500).min(1_000_000) as i32);
    d.set_when_done(completion_index(v.on_complete));
    d.set_complete_dialog(settings.show_complete_dialog);
    d.set_force_close(settings.force_shutdown);
    {
        let (mgr, w) = (ui.ctx.mgr.clone(), d.as_weak());
        d.on_limit_changed(move || {
            if let Some(d) = w.upgrade() {
                let kb = d.get_limit_kb().max(1) as u64;
                mgr.set_download_limit(id, d.get_limit_on().then_some(kb));
            }
        });
    }
    {
        let (mgr, w) = (ui.ctx.mgr.clone(), d.as_weak());
        d.on_completion_changed(move || {
            if let Some(d) = w.upgrade() {
                mgr.set_on_complete(id, completion_from(d.get_when_done()));
                let mut s = mgr.settings();
                if s.show_complete_dialog != d.get_complete_dialog() || s.force_shutdown != d.get_force_close() {
                    s.show_complete_dialog = d.get_complete_dialog();
                    s.force_shutdown = d.get_force_close();
                    let _ = mgr.set_settings(s);
                }
            }
        });
    }
    {
        let mgr = ui.ctx.mgr.clone();
        d.on_pause(move || mgr.pause(id));
    }
    {
        let mgr = ui.ctx.mgr.clone();
        d.on_resume(move || mgr.start(id));
    }
    {
        // "Cancel": stop the download and close the window
        let (mgr, map) = (ui.ctx.mgr.clone(), ui.progress.clone());
        d.on_cancel_download(move || {
            mgr.pause(id);
            close_progress(&map, id);
        });
    }
    {
        let map = ui.progress.clone();
        d.on_close_window(move || close_progress(&map, id));
    }
    {
        // "Hide next to the clock": the window goes, a small icon with the
        // progress comes next to the clock, the download goes on
        let (map, mgr, w) = (ui.progress.clone(), ui.ctx.mgr.clone(), d.as_weak());
        d.on_hide_to_tray(move || {
            let Some(d) = w.upgrade() else { return };
            let (tip, progress) = mgr.get(id).map(|v| tray_tip(&v)).unwrap_or_default();
            if tray::show_download(id, &tip, progress, (&t("Show download progress"), &t("Stop download"))) {
                let _ = d.hide();
            } else {
                // no icon could be made: at least out of the way
                d.window().set_minimized(true);
            }
            let _ = &map;
        });
    }
    {
        // "Hide details": the window gets small; "Show details": back to
        // the height it had
        // the choice is kept for the next windows
        let (w, mgr) = (d.as_weak(), ui.ctx.mgr.clone());
        d.on_details_toggled(move || {
            let (w, mgr) = (w.clone(), mgr.clone());
            let mut st = mgr.settings();
            if let Some(d) = w.upgrade() {
                if st.progress_details != d.get_show_details() {
                    st.progress_details = d.get_show_details();
                    let _ = mgr.set_settings(st);
                }
            }
            slint::Timer::single_shot(Duration::from_millis(1), move || {
                let Some(d) = w.upgrade() else { return };
                let scale = d.window().scale_factor();
                let now = d.window().size().to_logical(scale);
                let height = if d.get_show_details() {
                    let before = saved_size(&mgr, "progress").map(|s| s.1).unwrap_or(0.0);
                    before.max(d.get_content_height())
                } else {
                    d.get_content_height()
                };
                d.window().set_size(slint::LogicalSize::new(now.width, height.max(120.0)));
            });
        });
    }
    {
        // Alt+F4: the same
        let (map, mgr) = (ui.progress.clone(), ui.ctx.mgr.clone());
        d.window().on_close_requested(move || {
            stop_and_close(&map, &mgr, id);
            slint::CloseRequestResponse::HideWindow
        });
    }
    let segs = Rc::new(VecModel::<SegView>::default());
    d.set_segs(ModelRc::from(segs.clone()));
    fill_details(&d, &segs, &v);
    open_window!(d);
    ui.progress.borrow_mut().insert(id, (d, segs));
}

/// The X of a download's window: the download stops (it can be resumed).
fn stop_and_close(map: &Rc<RefCell<HashMap<u64, ProgressWin>>>, mgr: &Arc<Manager>, id: u64) {
    if mgr.get(id).is_some_and(|v| v.status != Status::Completed) {
        mgr.pause(id);
    }
    close_progress(map, id);
}

/// The hint and the bar of a hidden download's icon next to the clock.
fn tray_tip(v: &DownloadView) -> (String, Option<f32>) {
    let progress = v.total_size.filter(|t| *t > 0).map(|_| v.progress());
    let mut tip = match progress {
        Some(p) => format!("{:.0}%  {}", p * 100.0, v.file_name),
        None => format!("{}  {}", fmt_size(v.downloaded), v.file_name),
    };
    if v.waiting {
        tip.push_str(&format!("\n{}", t("Waiting for connection…")));
    } else if v.status.is_running() && v.speed > 0.0 {
        tip.push_str(&format!("\n{}/s", fmt_size(v.speed as u64)));
    } else if v.status == Status::Paused {
        tip.push_str(&format!("\n{}", t("Paused")));
    }
    (tip, progress)
}

fn close_progress(map: &Rc<RefCell<HashMap<u64, ProgressWin>>>, id: u64) {
    tray::remove_download(id);
    if let Some((d, _)) = map.borrow().get(&id) {
        let _ = d.hide();
    }
    let map = map.clone();
    slint::Timer::single_shot(Duration::from_millis(1), move || {
        map.borrow_mut().remove(&id);
    });
}

fn update_progress_windows(ui: &Ui) {
    let ids: Vec<u64> = ui.progress.borrow().keys().copied().collect();
    for id in ids {
        match ui.ctx.mgr.get(id) {
            // finished: the "complete" window takes over
            Some(v) if v.status == Status::Completed => close_progress(&ui.progress, id),
            Some(v) => {
                let (tip, progress) = tray_tip(&v);
                tray::update_download(id, &tip, progress);
                if let Some((d, segs)) = ui.progress.borrow().get(&id) {
                    fill_details(d, segs, &v);
                    // also on the window's taskbar button (seen when it is minimized)
                    winfx::taskbar_progress(d.window(), taskbar_state(&v));
                }
            }
            None => close_progress(&ui.progress, id),
        }
    }
}

/// The progress of `v` as the taskbar shows it.
fn taskbar_state(v: &DownloadView) -> winfx::TaskbarProgress {
    use winfx::TaskbarProgress as P;
    let amount = match v.total_size {
        Some(t) if t > 0 => Some((v.downloaded.min(t), t)),
        _ if v.parts_total > 0 => Some((v.parts_done.min(v.parts_total) as u64, v.parts_total as u64)),
        _ => None,
    };
    match (v.status, amount) {
        // waiting for the internet: yellow, like paused
        (_, Some((d, t))) if v.waiting => P::Paused(d, t),
        (Status::Failed, Some((d, t))) => P::Error(d, t),
        (Status::Failed, None) => P::Error(1, 1),
        (Status::Paused | Status::Queued, Some((d, t))) => P::Paused(d, t),
        (Status::Paused | Status::Queued, None) => P::Off,
        (_, Some((d, t))) => P::Normal(d, t),
        (_, None) => P::Busy,
    }
}

fn fill_details(d: &DetailsDialog, model: &VecModel<SegView>, v: &DownloadView) {
    let pct = v.progress();
    d.set_name(v.file_name.clone().into());
    d.set_kind(if v.kind == DownloadKind::Torrent { "torrent" } else { kind_for(&v.file_name) }.into());
    d.set_percent(format!("{:.0}%", pct * 100.0).into());
    d.set_url(v.url.clone().into());
    let status = match v.status {
        _ if v.waiting => t("No internet connection - it will continue by itself when it is back…"),
        Status::Downloading => t("Receiving data…"),
        Status::Assembling => t("Joining the parts…"),
        Status::Failed => t("Error"),
        other => t(other.label()),
    };
    d.set_status(status.into());
    d.set_size(v.total_size.map(fmt_size).unwrap_or_else(|| t("Unknown")).into());
    let done = if v.parts_total > 0 && v.total_size.is_none() {
        format!("{} ({}/{} parts)", fmt_size(v.downloaded), v.parts_done, v.parts_total)
    } else {
        format!("{} ({:.2} %)", fmt_size(v.downloaded), pct * 100.0)
    };
    d.set_downloaded(done.into());
    let speed = if v.speed > 0.0 { format!("{}/s", fmt_size(v.speed as u64)) } else { "—".into() };
    d.set_speed(speed.into());
    d.set_eta(v.eta_secs.map(fmt_duration).unwrap_or_else(|| "—".into()).into());
    d.set_resumable(t(if v.resumable { "Yes" } else { "No" }).into());
    d.set_resumable_ok(v.resumable);
    d.set_error(err_text(v.error.as_deref().unwrap_or_default()).into());
    d.set_progress(pct);
    d.set_state(state_of(v.status));

    // connections, in file order
    let base = v
        .segments
        .iter()
        .filter(|s| s.2 != u64::MAX)
        .map(|s| s.2)
        .max()
        .unwrap_or(0);
    let mut segs: Vec<(u64, u64, u64, bool)> = v.segments.clone();
    segs.sort_by_key(|s| s.0);
    let rows: Vec<SegView> = if base == 0 {
        Vec::new()
    } else {
        segs.iter()
            .enumerate()
            .map(|(i, (start, pos, end, active))| SegView {
                start: *start as f32 / base as f32,
                done: *pos as f32 / base as f32,
                number: (i + 1).to_string().into(),
                downloaded: fmt_size(pos.saturating_sub(*start)).into(),
                info: if pos >= end {
                    t("Finished").into()
                } else if *active && v.status.is_running() {
                    t("Receiving data…").into()
                } else {
                    t("Waiting").into()
                },
                busy: pos < end && *active && v.status.is_running(),
            })
            .collect()
    };
    update_model(model, rows);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn header_points_to_columns() {
        let w = [280.0, 30.0, 82.0];
        assert_eq!(super::column_at(&w, 10.0), 0);
        assert_eq!(super::column_at(&w, 38.0 + 279.0), 0);
        assert_eq!(super::column_at(&w, 38.0 + 281.0), 1);
        assert_eq!(super::column_at(&w, 5000.0), 2);
    }

    #[test]
    fn batch_patterns() {
        let l = expand_pattern("https://example.com/img*.jpg", false, "8", "10", 2).unwrap();
        assert_eq!(l, vec!["https://example.com/img08.jpg", "https://example.com/img09.jpg", "https://example.com/img10.jpg"]);
        let l = expand_pattern("https://example.com/*.pdf", true, "a", "c", 1).unwrap();
        assert_eq!(l.len(), 3);
        assert!(expand_pattern("https://example.com/x.pdf", false, "1", "2", 1).is_err());
    }
}

#[cfg(test)]
mod name_tests {
    use super::with_extension_of;
    #[test]
    fn typed_names_keep_the_extension() {
        assert_eq!(with_extension_of("My video", "Title [1080p].mp4"), "My video.mp4");
        assert_eq!(with_extension_of("My video.mkv", "Title.mp4"), "My video.mkv");
        assert_eq!(with_extension_of("Ep. 12 final", "a.rar"), "Ep. 12 final.rar");
        assert_eq!(with_extension_of("setup", ""), "setup");
        assert_eq!(with_extension_of("v1.2", "x.zip"), "v1.2");
    }
}
