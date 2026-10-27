//! Smaller windows: Columns, Category properties, Find, File Properties,
//! Duplicate link, shut down countdown, Tip of the day, Refresh download
//! address, and Export / Import of the list.

use crate::open_window;
use crate::*;
use swiftget_engine::config::{CategoryDef, ALL_COLUMNS, DEFAULT_COLUMNS};
use swiftget_engine::ef2::{self, Ef2Item};
use swiftget_engine::DetailsEdit;
use crate::i18n::{t, tf};

// ---------------------------------------------------------------- columns

pub(crate) fn open_columns(ui: &Ui) {
    let Some(d) = ui.columns.upgrade() else { return };
    d.set_cols(cols_from(&ui.ctx.mgr.settings()));
    open_window!(d);
}

fn cols_to_list(c: &Cols) -> Vec<String> {
    let flags = [c.q, c.size, c.status, c.eta, c.speed, c.last, c.desc, c.added, c.saveto, c.referer, c.parent];
    ALL_COLUMNS.iter().zip(flags).filter(|(_, on)| *on).map(|(k, _)| k.to_string()).collect()
}

// ---------------------------------------------------------------- categories

pub(crate) fn open_category(ui: &Ui, name: Option<String>) {
    let Some(d) = ui.category.upgrade() else { return };
    let s = ui.ctx.mgr.settings();
    *ui.editing_category.borrow_mut() = name.clone();
    d.set_message(SharedString::new());
    match name.as_deref() {
        None => {
            d.set_is_new(true);
            d.set_builtin(false);
            d.set_name(SharedString::new());
            d.set_exts(SharedString::new());
            d.set_sites(SharedString::new());
            d.set_folder(SharedString::new());
        }
        Some("General") => {
            d.set_is_new(false);
            d.set_builtin(true);
            d.set_name("General".into());
            d.set_exts("(every other file type)".into());
            d.set_sites(SharedString::new());
            d.set_folder(s.category_dirs.get("General").map(|p| p.display().to_string()).unwrap_or_default().into());
        }
        Some(n) => {
            let Some(c) = s.categories.iter().find(|c| c.name == n) else { return };
            d.set_is_new(false);
            d.set_builtin(c.builtin);
            d.set_name(c.name.clone().into());
            d.set_exts(c.exts.join(" ").into());
            d.set_sites(c.sites.join(" ").into());
            d.set_folder(s.category_dirs.get(n).map(|p| p.display().to_string()).unwrap_or_default().into());
        }
    }
    open_window!(d);
}

fn save_category(ui: &Ui) {
    let Some(d) = ui.category.upgrade() else { return };
    let mut s = ui.ctx.mgr.settings();
    let editing = ui.editing_category.borrow().clone();
    let name = d.get_name().trim().to_string();
    if name.is_empty() {
        d.set_message(t("Give the category a name").into());
        return;
    }
    let taken = name.eq_ignore_ascii_case("General")
        || s.categories.iter().any(|c| c.name.eq_ignore_ascii_case(&name) && Some(&c.name) != editing.as_ref());
    if taken && editing.as_deref() != Some("General") {
        d.set_message(t("A category with this name already exists").into());
        return;
    }
    let exts: Vec<String> = d
        .get_exts()
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .map(|e| e.trim().trim_start_matches("*.").trim_start_matches('.').to_lowercase())
        .filter(|e| !e.is_empty() && !e.starts_with('('))
        .collect();
    let sites: Vec<String> = d.get_sites().split_whitespace().map(|x| x.to_lowercase()).collect();
    let folder = d.get_folder().trim().to_string();
    match editing.as_deref() {
        Some("General") => {}
        Some(old) => {
            if let Some(c) = s.categories.iter_mut().find(|c| c.name == old) {
                c.name = name.clone();
                c.exts = exts.clone();
                c.sites = sites;
            }
            // a type belongs to one category only
            for c in s.categories.iter_mut().filter(|c| c.name != name) {
                c.exts.retain(|e| !exts.contains(e));
            }
            if old != name {
                if let Some(p) = s.category_dirs.remove(old) {
                    s.category_dirs.insert(name.clone(), p);
                }
            }
        }
        None => {
            for c in s.categories.iter_mut() {
                c.exts.retain(|e| !exts.contains(e));
            }
            s.categories.push(CategoryDef { name: name.clone(), exts, sites, builtin: false });
        }
    }
    if folder.is_empty() {
        s.category_dirs.remove(&name);
    } else {
        s.category_dirs.insert(name.clone(), PathBuf::from(folder));
    }
    if let Err(e) = ui.ctx.mgr.set_settings(s) {
        d.set_message(format!("{e:#}").into());
        return;
    }
    ui.ctx.mgr.recategorize();
    let _ = d.hide();
    refresh(ui);
}

pub(crate) fn delete_category(ui: &Ui, name: &str) {
    let mut s = ui.ctx.mgr.settings();
    let Some(c) = s.categories.iter().find(|c| c.name == name) else { return };
    if c.builtin || name == "General" {
        message(&t("This category comes with Ezoo Download and can't be deleted (you can change its file types in Properties)."));
        return;
    }
    if !ask(&tf("Delete the category “{}”?\nIts files stay where they are and move to their file type's category.", &[&name])) {
        return;
    }
    s.categories.retain(|c| c.name != name);
    s.category_dirs.remove(name);
    let _ = ui.ctx.mgr.set_settings(s);
    ui.ctx.mgr.recategorize();
    if let Some(m) = ui.ctx.main.upgrade() {
        m.set_view("all".into());
    }
    refresh(ui);
}

// ---------------------------------------------------------------- find

pub(crate) fn open_find(ui: &Ui) {
    if let Some(d) = ui.find.upgrade() {
        d.set_message(SharedString::new());
        open_window!(d);
    }
}

/// Select the next download (after the selected one) that matches.
pub(crate) fn find_next(ui: &Ui) {
    let (Some(d), Some(m)) = (ui.find.upgrade(), ui.ctx.main.upgrade()) else { return };
    let needle = d.get_text().to_string();
    if needle.trim().is_empty() {
        open_find(ui);
        return;
    }
    let case = d.get_match_case();
    let whole = d.get_whole();
    let norm = |s: &str| if case { s.to_string() } else { s.to_lowercase() };
    let needle = norm(needle.trim());
    let hit = |hay: &str| {
        let hay = norm(hay);
        if whole {
            hay == needle
        } else {
            hay.contains(&needle)
        }
    };
    // search in the whole list, in the order shown
    if m.get_view() != "all" && m.get_search().is_empty() {
        m.set_view("all".into());
        refresh(ui);
    }
    let ids: Vec<u64> = (0..ui.rows.row_count()).filter_map(|i| ui.rows.row_data(i)).map(|r| r.id as u64).collect();
    let start = ids.iter().position(|id| *id as i32 == m.get_selected_id()).map(|p| p + 1).unwrap_or(0);
    for k in 0..ids.len() {
        let id = ids[(start + k) % ids.len()];
        let Some(v) = ui.ctx.mgr.get(id) else { continue };
        let found = (d.get_in_name() && hit(&v.file_name))
            || (d.get_in_desc() && hit(v.description.as_deref().unwrap_or("")))
            || (d.get_in_address()
                && (hit(&v.url) || hit(v.referer()) || hit(v.parent_page.as_deref().unwrap_or(""))));
        if found {
            select_single(ui, id);
            d.set_message(tf("Found: {}", &[&v.file_name]).into());
            refresh(ui);
            return;
        }
    }
    d.set_message(t("Not found").into());
}

// ---------------------------------------------------------------- properties

pub(crate) fn open_properties(ui: &Ui, id: u64) {
    let (Some(d), Some(v)) = (ui.props.upgrade(), ui.ctx.mgr.get(id)) else { return };
    ui.props_id.set(Some(id));
    let done = v.status == Status::Completed;
    d.set_file_name(v.file_name.clone().into());
    d.set_kind(kind_for(&v.file_name).into());
    d.set_ext(filename::extension(&v.file_name).to_uppercase().into());
    let ext = filename::extension(&v.file_name).to_uppercase();
    d.set_type_text(
        if ext.is_empty() { t(&v.category) } else { tf("{} file  ·  {}", &[&ext, &t(&v.category)]) }.into(),
    );
    d.set_status(
        match v.status {
            Status::Downloading | Status::Paused if v.total_size.is_some() => {
                format!("{} ({:.1}%)", t(v.status.label()), v.progress() * 100.0)
            }
            s => t(s.label()),
        }
        .into(),
    );
    d.set_size(
        match v.total_size {
            Some(t) => format!("{} ({} bytes)", fmt_size(t), t),
            None => t("Unknown"),
        }
        .into(),
    );
    d.set_added(fmt_time(v.added_at).into());
    d.set_last_try(v.finished_at.or(v.last_try).map(fmt_time).unwrap_or_default().into());
    d.set_result(
        match (&v.error, v.status) {
            (Some(e), _) => crate::err_text(e),
            (None, Status::Completed) => t("The file was downloaded successfully"),
            _ => String::new(),
        }
        .into(),
    );
    d.set_done(done);
    d.set_save_to(v.save_path.display().to_string().into());
    d.set_url(v.url.clone().into());
    d.set_referer(v.referer().to_string().into());
    d.set_description(v.description.clone().unwrap_or_default().into());
    d.set_use_login(v.headers.auth.is_some());
    d.set_user(v.headers.auth.as_ref().map(|a| a.0.clone()).unwrap_or_default().into());
    d.set_password(v.headers.auth.as_ref().map(|a| a.1.clone()).unwrap_or_default().into());
    d.set_message(SharedString::new());
    open_window!(d);
}

fn save_properties(ui: &Ui) {
    let (Some(d), Some(id)) = (ui.props.upgrade(), ui.props_id.get()) else { return };
    let Some(v) = ui.ctx.mgr.get(id) else { return };
    let auth = (d.get_use_login() && !d.get_user().trim().is_empty())
        .then(|| (d.get_user().trim().to_string(), d.get_password().to_string()));
    let edit = DetailsEdit {
        url: (v.status != Status::Completed).then(|| d.get_url().trim().to_string()),
        referer: Some(d.get_referer().trim().to_string()),
        description: Some(d.get_description().trim().to_string()),
        auth,
    };
    if let Err(e) = ui.ctx.mgr.update_details(id, edit) {
        d.set_message(format!("{e:#}").into());
        return;
    }
    let mut target = PathBuf::from(d.get_save_to().trim());
    // a name typed without its extension keeps the file's own
    if let Some(n) = target.file_name().map(|n| crate::with_extension_of(&n.to_string_lossy(), &v.file_name)) {
        target.set_file_name(n);
    }
    if !target.as_os_str().is_empty() && target != v.save_path {
        let result = if v.status == Status::Completed {
            // on its own thread: a move to another disk is a copy
            let mgr = ui.ctx.mgr.clone();
            std::thread::spawn(move || {
                if let Err(e) = mgr.move_file(id, &target) {
                    let text = format!("{e:#}");
                    let _ = slint::invoke_from_event_loop(move || crate::message(&text));
                }
            });
            Ok(())
        } else {
            let dir = target.parent().map(Path::to_path_buf).unwrap_or_default();
            let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if name.is_empty() {
                Err(anyhow::anyhow!("type a file name"))
            } else {
                ui.ctx.mgr.change_target(id, &dir, &name)
            }
        };
        if let Err(e) = result {
            d.set_message(tf("Could not change where it is saved: {}", &[&format!("{e:#}")]).into());
            return;
        }
    }
    let _ = d.hide();
    refresh(ui);
}

// ---------------------------------------------------------------- duplicate link

pub(crate) fn ask_duplicate(ui: &Ui, p: PendingAdd) {
    let Some(d) = ui.dup.upgrade() else { return };
    let v = ui.ctx.mgr.get(p.existing);
    d.set_file_name(v.as_ref().map(|v| v.file_name.clone()).unwrap_or_default().into());
    d.set_existing_status(v.as_ref().map(|v| v.status.label()).unwrap_or("").into());
    d.set_choice(1);
    d.set_remember(false);
    *ui.pending.borrow_mut() = Some(p);
    open_window!(d);
}

// ---------------------------------------------------------------- shut down countdown

const COUNTDOWN: i32 = 30;

pub(crate) fn start_countdown(ui: &Ui, action: AfterDone) {
    let Some(d) = ui.countdown.upgrade() else { return };
    let text = match action {
        AfterDone::Shutdown => "shut down the computer",
        AfterDone::Hibernate => "hibernate the computer",
        _ => "put the computer to sleep",
    };
    d.set_action_text(t(text).into());
    d.set_seconds_total(COUNTDOWN);
    d.set_seconds_left(COUNTDOWN);
    *ui.power.borrow_mut() = Some((action, COUNTDOWN));
    open_window!(d);
}

pub(crate) fn tick_countdown(ui: &Ui) {
    let left = {
        let mut p = ui.power.borrow_mut();
        let Some((_, left)) = p.as_mut() else { return };
        *left -= 1;
        *left
    };
    if let Some(d) = ui.countdown.upgrade() {
        d.set_seconds_left(left.max(0));
    }
    if left <= 0 {
        power_now(ui);
    }
}

fn power_now(ui: &Ui) {
    let Some((action, _)) = ui.power.borrow_mut().take() else { return };
    if let Some(d) = ui.countdown.upgrade() {
        let _ = d.hide();
    }
    // save the list first: the downloads resume after the restart
    ui.ctx.mgr.shutdown();
    let force = ui.ctx.mgr.settings().force_shutdown;
    swiftget_engine::system::power(action, force);
}

// ---------------------------------------------------------------- tips

/// http:// or https:// with a host: safe to hand to the browser.
pub(crate) fn is_web_address(s: &str) -> bool {
    url::Url::parse(s.trim())
        .map(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some_and(|h| !h.is_empty()))
        .unwrap_or(false)
}

/// What the newest version brings (shown once after an automatic update).
const WHATS_NEW: [&str; 11] = [
    "A click on a file link in the browser goes straight to Ezoo Download: the browser's own Save As window no longer opens (Brave too).",
    "Changing the folder or the name in \"Download File Info\" no longer gives a small broken file, and a name typed without its extension keeps it.",
    "Big files keep going by themselves when the server has trouble: Ezoo Download tries again later on its own.",
    "Torrents: a .torrent link in the browser opens the torrent window at once, .torrent files and magnet links open with Ezoo Download, and a .torrent file can be dropped on the window.",
    "Removing downloads can delete their files from the disk too (if you tick it), and the question can be turned off.",
    "Ctrl+A, Ctrl+N and the other shortcuts work with the Arabic keyboard too, and files are selected with the mouse in both directions.",
    "The download window is smaller, never gets wider for a long name, and its title bar is like the main window's.",
    "The video button in the browser hides in full screen (or not: the add-in's window), stays closed when you close it, and streams show their quality first.",
    "Firefox: the add-in is signed again and updates by itself.",
    "Playlists have 240p and 144p, the MP3 quality can be chosen, and the subtitles of videos can be downloaded too (Options > Downloads).",
    "The toolbar grows with bigger text, so the names are never cut.",
];

thread_local! {
    static WHATS_NEW_WIN: std::cell::RefCell<Option<WhatsNewDialog>> = const { std::cell::RefCell::new(None) };
}

/// After an update: what is new, in its own window (right to left in
/// Arabic), with a word about supporting the free program.
pub(crate) fn whats_new(ui: &Ui) {
    let Ok(d) = WhatsNewDialog::new() else { return };
    crate::set_theme_of(d.global::<Theme>(), &crate::Look::of(&ui.ctx.mgr.settings()));
    d.set_rtl(crate::i18n::is_arabic());
    d.set_heading(tf("Ezoo Download was updated to version {} by itself.", &[&env!("CARGO_PKG_VERSION")]).into());
    let items: Vec<SharedString> = WHATS_NEW.iter().map(|l| t(l).into()).collect();
    d.set_items(ModelRc::new(VecModel::from(items)));
    d.on_open_link(|u| {
        if u.starts_with("https://") {
            let _ = open::that(u.as_str());
        }
    });
    let w = d.as_weak();
    d.on_close_window(move || {
        if let Some(d) = w.upgrade() {
            let _ = d.hide();
        }
        // let the window go after this click
        slint::Timer::single_shot(std::time::Duration::from_millis(1), || WHATS_NEW_WIN.with(|x| *x.borrow_mut() = None));
    });
    open_window!(d);
    WHATS_NEW_WIN.with(|x| *x.borrow_mut() = Some(d));
}

const TIPS: [&str; 14] = [
    "Copy a download link anywhere (Ctrl+C) and Ezoo Download offers to download it.",
    "Play a video on a web page and click \"Download this video\" above it to choose the quality.",
    "Right click a web page in Chrome or Edge and choose \"Download all links with Ezoo Download\" to pick files from the page.",
    "Every download uses up to 32 connections. If a server is slow with many, add it to Options > Connection > Exceptions.",
    "Make a queue (Queues > New queue) and set \"1 file at the same time\" to download files one after another.",
    "The Scheduler can start a queue at night and shut down the computer when everything is finished.",
    "Downloads > When all downloads finish lets you shut down, sleep or hibernate the computer after the last file.",
    "The speed limiter on the toolbar keeps your internet usable for other things while downloading.",
    "Add Batch > \"Address with a pattern\" downloads img001.jpg … img100.jpg from one address with a *.",
    "An old link expired? Right click the download > \"Refresh download address\" and continue from where it stopped.",
    "Right click a category in the sidebar > Properties to change its file types and its own folder.",
    "View > Columns shows more columns, like Date Added, Save To and Referer.",
    "Tasks > Run site grabber collects every image, video or archive of a web site.",
    "Hold Alt while clicking a link in the browser to let the browser download it by itself.",
];

/// The tip at start, once no message box is open (they must not mix).
pub(crate) fn show_tip_when_free(ui: Ui) {
    if modal_open() {
        slint::Timer::single_shot(std::time::Duration::from_millis(700), move || show_tip_when_free(ui));
        return;
    }
    show_tip(&ui, false);
}

pub(crate) fn show_tip(ui: &Ui, from_menu: bool) {
    let Some(d) = ui.tips.upgrade() else { return };
    let mut s = ui.ctx.mgr.settings();
    let i = s.tip_index as usize % TIPS.len();
    d.set_tip(t(TIPS[i]).into());
    d.set_counter(tf("Tip {} of {}", &[&(i + 1), &TIPS.len()]).into());
    d.set_show_on_start(s.show_tips);
    if !from_menu {
        // the next start shows the next tip
        s.tip_index = ((i + 1) % TIPS.len()) as u32;
        let _ = ui.ctx.mgr.set_settings(s);
    }
    open_window!(d);
}

// ---------------------------------------------------------------- refresh download address

pub(crate) fn open_refresh(ui: &Ui, id: u64) {
    let (Some(d), Some(v)) = (ui.refresh_dlg.upgrade(), ui.ctx.mgr.get(id)) else { return };
    if v.status == Status::Completed {
        return;
    }
    ui.ctx.mgr.pause(id);
    ui.refresh_id.set(Some(id));
    d.set_file_name(v.file_name.clone().into());
    d.set_page(v.parent_page.clone().unwrap_or_default().into());
    d.set_waiting(true);
    d.set_new_url(SharedString::new());
    open_window!(d);
}

/// The browser sent a download while we were waiting for a new address.
pub(crate) fn refresh_received(ui: &Ui, id: u64, url: &str, headers: RequestHeaders) {
    ui.refresh_id.set(None);
    if let Some(d) = ui.refresh_dlg.upgrade() {
        let _ = d.hide();
    }
    ui.ctx.mgr.set_url(id, url, Some(headers));
    ui.ctx.mgr.start(id);
    open_progress(ui, id);
    refresh(ui);
}

// ---------------------------------------------------------------- export / import

pub(crate) fn export_list(mgr: &Arc<Manager>, kind: &str) {
    let ef2 = kind == "ef2";
    let Some(path) = crate::winfx::file_box()
        .set_file_name(if ef2 { "ezoo-download-list.ef2" } else { "ezoo-download-links.txt" })
        .add_filter(if ef2 { "Download list" } else { "Text file" }, &[if ef2 { "ef2" } else { "txt" }])
        .save_file()
    else {
        return;
    };
    let text = if ef2 {
        let items: Vec<Ef2Item> = mgr
            .snapshot()
            .into_iter()
            .map(|v| Ef2Item {
                url: v.parent_page.clone().filter(|_| v.kind == DownloadKind::Dash).unwrap_or(v.url.clone()),
                referer: v.headers.referer.clone(),
                cookies: v.headers.cookies.clone(),
                user_agent: v.headers.user_agent.clone(),
                file_name: Some(v.file_name.clone()),
                description: v.description.clone(),
            })
            .collect();
        ef2::write(&items)
    } else {
        mgr.all_urls().join("\r\n")
    };
    if let Err(e) = std::fs::write(&path, text) {
        message(&tf("Could not save the list: {}", &[&e]));
    }
}

pub(crate) fn import_list(ui: &Ui, kind: &str) {
    let ef2 = kind == "ef2";
    let Some(path) = crate::winfx::file_box()
        .add_filter(if ef2 { "Download list" } else { "Text file" }, &[if ef2 { "ef2" } else { "txt" }])
        .pick_file()
    else {
        return;
    };
    let text = match std::fs::read(&path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => return message(&tf("Could not read the file: {}", &[&e])),
    };
    let items = ef2::parse(&text);
    if items.is_empty() {
        return message(&t("No download addresses were found in this file."));
    }
    let mut added = 0;
    for it in items {
        if ui.ctx.mgr.find_url(&it.url).is_some() {
            continue;
        }
        ui.ctx.mgr.add(NewDownload {
            url: it.url,
            file_name: it.file_name,
            headers: RequestHeaders {
                referer: it.referer,
                cookies: it.cookies,
                user_agent: it.user_agent,
                ..Default::default()
            },
            description: it.description,
            queue: Some(MAIN_QUEUE),
            start: false,
            ..Default::default()
        });
        added += 1;
    }
    refresh(ui);
    message(&tf(
        "{} download(s) were added to the main download queue.\nStart them with Start Queue, or one by one with Resume.",
        &[&added],
    ));
}

// ---------------------------------------------------------------- wiring

pub(crate) fn wire(ui: &Ui) {
    if let Some(d) = ui.columns.upgrade() {
        {
            let ui = ui.clone();
            d.on_save(move || {
                let Some(d) = ui.columns.upgrade() else { return };
                let mut s = ui.ctx.mgr.settings();
                s.columns = cols_to_list(&d.get_cols());
                let _ = ui.ctx.mgr.set_settings(s.clone());
                apply_view_settings(&ui, &s);
                let _ = d.hide();
            });
        }
        {
            let w = d.as_weak();
            d.on_reset(move || {
                if let Some(d) = w.upgrade() {
                    let s = Settings { columns: DEFAULT_COLUMNS.iter().map(|c| c.to_string()).collect(), ..Default::default() };
                    d.set_cols(cols_from(&s));
                }
            });
        }
        {
            let w = d.as_weak();
            d.on_cancel(move || {
                if let Some(d) = w.upgrade() {
                    let _ = d.hide();
                }
            });
        }
    }
    if let Some(d) = ui.category.upgrade() {
        {
            let w = d.as_weak();
            d.on_browse(move || {
                let Some(d) = w.upgrade() else { return };
                if let Some(dir) = crate::winfx::file_box().pick_folder() {
                    d.set_folder(dir.display().to_string().into());
                }
            });
        }
        {
            let ui = ui.clone();
            d.on_save(move || save_category(&ui));
        }
        {
            let w = d.as_weak();
            d.on_cancel(move || {
                if let Some(d) = w.upgrade() {
                    let _ = d.hide();
                }
            });
        }
    }
    if let Some(d) = ui.find.upgrade() {
        {
            let ui = ui.clone();
            d.on_find_next(move || find_next(&ui));
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
    if let Some(d) = ui.props.upgrade() {
        {
            let w = d.as_weak();
            d.on_browse(move || {
                let Some(d) = w.upgrade() else { return };
                let current = PathBuf::from(d.get_save_to().as_str());
                let mut dlg = crate::winfx::file_box();
                if let Some(dir) = current.parent().filter(|p| p.exists()) {
                    dlg = dlg.set_directory(dir);
                }
                if let Some(name) = current.file_name() {
                    dlg = dlg.set_file_name(name.to_string_lossy());
                }
                if let Some(p) = dlg.save_file() {
                    d.set_save_to(p.display().to_string().into());
                }
            });
        }
        {
            let ui = ui.clone();
            d.on_open_file(move || {
                if let Some(v) = ui.props_id.get().and_then(|id| ui.ctx.mgr.get(id)) {
                    crate::open_download(&v.save_path);
                }
            });
        }
        {
            let ui = ui.clone();
            d.on_save(move || save_properties(&ui));
        }
        {
            let w = d.as_weak();
            d.on_cancel(move || {
                if let Some(d) = w.upgrade() {
                    let _ = d.hide();
                }
            });
        }
    }
    if let Some(d) = ui.dup.upgrade() {
        {
            let ui = ui.clone();
            d.on_ok(move || {
                let Some(d) = ui.dup.upgrade() else { return };
                let choice = d.get_choice().clamp(1, 3) as u32;
                if d.get_remember() {
                    let mut s = ui.ctx.mgr.settings();
                    s.duplicate_action = choice;
                    let _ = ui.ctx.mgr.set_settings(s);
                }
                let _ = d.hide();
                if let Some(p) = ui.pending.borrow_mut().take() {
                    apply_duplicate(&ui, p, choice);
                }
            });
        }
        {
            let ui = ui.clone();
            d.on_cancel(move || {
                ui.pending.borrow_mut().take();
                if let Some(d) = ui.dup.upgrade() {
                    let _ = d.hide();
                }
            });
        }
    }
    if let Some(d) = ui.countdown.upgrade() {
        {
            let ui = ui.clone();
            d.on_now(move || power_now(&ui));
        }
        {
            let ui = ui.clone();
            d.on_stop(move || {
                ui.power.borrow_mut().take();
                if let Some(d) = ui.countdown.upgrade() {
                    let _ = d.hide();
                }
            });
        }
        {
            let ui = ui.clone();
            d.window().on_close_requested(move || {
                ui.power.borrow_mut().take();
                slint::CloseRequestResponse::HideWindow
            });
        }
    }
    if let Some(d) = ui.tips.upgrade() {
        {
            let ui = ui.clone();
            d.on_next(move || {
                let mut s = ui.ctx.mgr.settings();
                s.tip_index = (s.tip_index + 1) % TIPS.len() as u32;
                let _ = ui.ctx.mgr.set_settings(s);
                show_tip(&ui, true);
            });
        }
        {
            let ui = ui.clone();
            d.on_close_window(move || {
                let Some(d) = ui.tips.upgrade() else { return };
                let mut s = ui.ctx.mgr.settings();
                if s.show_tips != d.get_show_on_start() {
                    s.show_tips = d.get_show_on_start();
                    let _ = ui.ctx.mgr.set_settings(s);
                }
                let _ = d.hide();
            });
        }
    }
    if let Some(d) = ui.refresh_dlg.upgrade() {
        {
            let w = d.as_weak();
            d.on_open_page(move || {
                if let Some(d) = w.upgrade() {
                    let page = d.get_page().to_string();
                    // only a web page: the address came from the browser or
                    // a list file and must never start a program
                    if is_web_address(&page) {
                        let _ = open::that(page.trim());
                    }
                }
            });
        }
        {
            let ui = ui.clone();
            d.on_use_url(move || {
                let Some(d) = ui.refresh_dlg.upgrade() else { return };
                let url = d.get_new_url().trim().to_string();
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return;
                }
                if let Some(id) = ui.refresh_id.get() {
                    refresh_received(&ui, id, &url, ui.ctx.mgr.get(id).map(|v| v.headers).unwrap_or_default());
                }
            });
        }
        {
            let ui = ui.clone();
            d.on_stop_waiting(move || {
                ui.refresh_id.set(None);
                if let Some(d) = ui.refresh_dlg.upgrade() {
                    let _ = d.hide();
                }
            });
        }
        {
            let ui = ui.clone();
            d.window().on_close_requested(move || {
                ui.refresh_id.set(None);
                slint::CloseRequestResponse::HideWindow
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_web_address;

    #[test]
    fn only_web_pages_are_opened() {
        assert!(is_web_address("https://example.com/page"));
        assert!(is_web_address(" http://example.com "));
        for bad in [
            "C:\\Windows\\System32\\calc.exe",
            "\\\\server\\share\\x.exe",
            "file:///C:/x.exe",
            "javascript:alert(1)",
            "ms-settings:",
            "https://",
            "",
        ] {
            assert!(!is_web_address(bad), "{bad}");
        }
    }
}
