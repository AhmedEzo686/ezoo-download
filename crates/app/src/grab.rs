//! Site Grabber and "Download all links" (both use the same window).

use crate::open_window;
use crate::*;
use std::sync::atomic::{AtomicBool, Ordering};
use swiftget_engine::grabber::{self, FoundFile, GrabOptions, GrabProgress};
use tokio_util::sync::CancellationToken;
use crate::i18n::{t, tf};

#[derive(Clone)]
struct Item {
    checked: bool,
    url: String,
    name: String,
    ext: String,
    kind: &'static str,
    text: String,
    page: String,
    is_page: bool,
}

#[derive(Default)]
pub(crate) struct GrabUi {
    all: Vec<Item>,
    /// rows on screen -> index in `all`
    shown: Vec<usize>,
    model: Option<Rc<VecModel<LinkRow>>>,
    incoming: Arc<Mutex<Vec<FoundFile>>>,
    progress: Option<Arc<GrabProgress>>,
    cancel: Option<CancellationToken>,
    finished: Arc<AtomicBool>,
    exploring: bool,
    /// links mode: the page and its cookies
    headers: RequestHeaders,
}

fn item_from(url: &str, text: &str, page: &str) -> Item {
    let name = filename::from_url(url).unwrap_or_else(|| "download".into());
    let ext = filename::extension(&name);
    let is_page = grabber::is_page_ext(&ext);
    Item {
        checked: !is_page,
        url: url.to_string(),
        kind: if is_page { "text" } else { kind_for(&name) },
        ext: ext.to_uppercase(),
        name,
        text: text.trim().chars().take(200).collect(),
        page: page.to_string(),
        is_page,
    }
}

fn type_ok(it: &Item, filter: i32) -> bool {
    match filter {
        1 => it.kind == "image",
        2 => it.kind == "video",
        3 => it.kind == "audio",
        4 => matches!(it.kind, "archive" | "exe" | "apk" | "disc"),
        5 => matches!(it.kind, "pdf" | "doc" | "sheet" | "slides" | "text") && !it.is_page,
        6 => it.is_page,
        _ => !it.is_page,
    }
}

fn row(it: &Item) -> LinkRow {
    LinkRow {
        checked: it.checked,
        name: it.name.clone().into(),
        kind: it.kind.into(),
        ext: it.ext.clone().into(),
        text: it.text.clone().into(),
        url: it.url.clone().into(),
    }
}

/// Rebuild the visible rows from the filters.
fn show(ui: &Ui) {
    let Some(d) = ui.grabber.upgrade() else { return };
    let filter = d.get_filter().to_lowercase();
    let type_filter = d.get_type_filter();
    let mut g = ui.grab_state.borrow_mut();
    let shown: Vec<usize> = g
        .all
        .iter()
        .enumerate()
        .filter(|(_, it)| type_ok(it, type_filter))
        .filter(|(_, it)| {
            filter.is_empty() || it.name.to_lowercase().contains(&filter) || it.url.to_lowercase().contains(&filter)
        })
        .map(|(i, _)| i)
        .collect();
    let rows: Vec<LinkRow> = shown.iter().map(|i| row(&g.all[*i])).collect();
    g.shown = shown;
    let model = g.model.get_or_insert_with(|| Rc::new(VecModel::default())).clone();
    let checked = g.all.iter().filter(|i| i.checked).count();
    drop(g);
    update_model(&model, rows);
    d.set_links(ModelRc::from(model));
    d.set_checked_text(tf("{} file(s) checked", &[&checked]).into());
}

fn reset(ui: &Ui, d: &GrabberDialog, links_mode: bool) {
    let mut g = ui.grab_state.borrow_mut();
    if let Some(c) = g.cancel.take() {
        c.cancel();
    }
    g.all.clear();
    g.shown.clear();
    g.exploring = false;
    g.progress = None;
    g.headers = RequestHeaders::default();
    g.incoming = Arc::new(Mutex::new(Vec::new()));
    drop(g);
    let s = ui.ctx.mgr.settings();
    d.set_links_mode(links_mode);
    d.set_exploring(false);
    d.set_message(SharedString::new());
    d.set_filter(SharedString::new());
    d.set_type_filter(0);
    d.set_queue_names(queue_names(&s));
    d.set_queue_index(0);
    d.set_folder(SharedString::new());
    d.set_progress_text(SharedString::new());
}

/// Tasks > Run site grabber.
pub(crate) fn open_grabber(ui: &Ui) {
    let Some(d) = ui.grabber.upgrade() else { return };
    if d.window().is_visible() && !d.get_links_mode() {
        open_window!(d);
        return;
    }
    reset(ui, &d, false);
    show(ui);
    open_window!(d);
}

/// "Download all links with Ezoo Download" from the browser.
pub(crate) fn open_links(ui: &Ui, l: api::LinksRequest) {
    let Some(d) = ui.grabber.upgrade() else { return };
    reset(ui, &d, true);
    {
        let mut g = ui.grab_state.borrow_mut();
        g.headers = RequestHeaders {
            referer: (!l.page.is_empty()).then(|| l.page.clone()),
            cookies: l.cookies.clone(),
            user_agent: l.user_agent.clone(),
            ..Default::default()
        };
        let mut seen = std::collections::HashSet::new();
        for link in &l.links {
            if seen.insert(link.url.clone()) {
                g.all.push(item_from(&link.url, &link.text, &l.page));
            }
        }
    }
    d.set_start_url(l.page.clone().into());
    show(ui);
    bring_to_front(&ui.ctx);
    open_window!(d);
}

/// New files from the explorer, and its counters (10 times a second).
pub(crate) fn poll(ui: &Ui) {
    let (new, progress, finished) = {
        let g = ui.grab_state.borrow();
        if !g.exploring {
            return;
        }
        let new: Vec<FoundFile> = std::mem::take(&mut *g.incoming.lock().unwrap());
        (new, g.progress.clone(), g.finished.load(Ordering::Relaxed))
    };
    let Some(d) = ui.grabber.upgrade() else { return };
    if !new.is_empty() {
        let mut g = ui.grab_state.borrow_mut();
        for f in new {
            g.all.push(item_from(&f.url, &f.text, &f.page));
        }
        drop(g);
        show(ui);
    }
    if let Some(p) = progress {
        let current = p.current.lock().map(|c| c.clone()).unwrap_or_default();
        let text = tf(
            "{} of {} pages explored  ·  {} files found",
            &[&p.pages_done.load(Ordering::Relaxed), &p.pages_queued.load(Ordering::Relaxed), &p.files.load(Ordering::Relaxed)],
        ) + &if finished { t("  ·  done") } else { format!("  ·  {current}") };
        d.set_progress_text(text.into());
    }
    if finished {
        ui.grab_state.borrow_mut().exploring = false;
        d.set_exploring(false);
    }
}

pub(crate) fn wire(ui: &Ui) {
    let Some(d) = ui.grabber.upgrade() else { return };
    {
        let w = d.as_weak();
        d.on_template_changed(move |t| {
            if let Some(d) = w.upgrade() {
                if t != 6 {
                    d.set_exts(grabber::template_exts(t).join(" ").into());
                }
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_explore(move || {
            let Some(d) = ui.grabber.upgrade() else { return };
            let start = d.get_start_url().trim().to_string();
            if !(start.starts_with("http://") || start.starts_with("https://")) || url::Url::parse(&start).is_err() {
                d.set_message(t("Type the address of the start page (http:// or https://)").into());
                return;
            }
            d.set_message(SharedString::new());
            let exts: Vec<String> = if d.get_template() == 6 {
                d.get_exts().split_whitespace().map(|e| e.trim_start_matches('.').to_lowercase()).collect()
            } else {
                grabber::template_exts(d.get_template())
            };
            let s = ui.ctx.mgr.settings();
            let opts = GrabOptions {
                start_url: start.clone(),
                depth: d.get_depth().clamp(0, 10) as u32,
                other_sites: d.get_other_sites(),
                no_parent: d.get_no_parent(),
                exts,
                include: Vec::new(),
                exclude: Vec::new(),
                max_pages: 2000,
                parallel: d.get_parallel().clamp(1, 10) as usize,
                headers: RequestHeaders { auth: s.login_for(&start), ..Default::default() },
                user_agent: s.user_agent.clone(),
            };
            let progress = Arc::new(GrabProgress::default());
            let cancel = CancellationToken::new();
            let finished = Arc::new(AtomicBool::new(false));
            let incoming = Arc::new(Mutex::new(Vec::new()));
            {
                let mut g = ui.grab_state.borrow_mut();
                if let Some(c) = g.cancel.take() {
                    c.cancel();
                }
                g.all.clear();
                g.progress = Some(progress.clone());
                g.cancel = Some(cancel.clone());
                g.finished = finished.clone();
                g.incoming = incoming.clone();
                g.exploring = true;
            }
            show(&ui);
            d.set_exploring(true);
            d.set_progress_text(t("Starting…").into());
            let client = ui.ctx.mgr.client();
            ui.ctx.rt.spawn(async move {
                let sink = incoming.clone();
                grabber::crawl(client, opts, progress, cancel, move |f| sink.lock().unwrap().push(f)).await;
                finished.store(true, Ordering::Relaxed);
            });
        });
    }
    {
        let ui = ui.clone();
        d.on_stop_explore(move || {
            if let Some(c) = ui.grab_state.borrow().cancel.clone() {
                c.cancel();
            }
            ui.grab_state.borrow().finished.store(true, Ordering::Relaxed);
        });
    }
    {
        let ui = ui.clone();
        d.on_toggle(move |i| {
            {
                let mut g = ui.grab_state.borrow_mut();
                if let Some(&idx) = g.shown.get(i.max(0) as usize) {
                    g.all[idx].checked = !g.all[idx].checked;
                }
            }
            show(&ui);
        });
    }
    {
        let ui = ui.clone();
        d.on_check_all(move |on| {
            {
                let mut g = ui.grab_state.borrow_mut();
                let shown = g.shown.clone();
                for idx in shown {
                    g.all[idx].checked = on;
                }
            }
            show(&ui);
        });
    }
    {
        let ui = ui.clone();
        d.on_filter_changed(move || show(&ui));
    }
    {
        let (w, mgr) = (d.as_weak(), ui.ctx.mgr.clone());
        d.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            if let Some(dir) = rfd::FileDialog::new().set_directory(mgr.settings().download_dir).pick_folder() {
                d.set_folder(dir.display().to_string().into());
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_download(move || {
            let Some(d) = ui.grabber.upgrade() else { return };
            let (items, headers) = {
                let g = ui.grab_state.borrow();
                (g.all.iter().filter(|i| i.checked).cloned().collect::<Vec<_>>(), g.headers.clone())
            };
            if items.is_empty() {
                d.set_message(t("Check the files you want first").into());
                return;
            }
            let s = ui.ctx.mgr.settings();
            let queue = s.queues.get(d.get_queue_index().max(0) as usize).map(|q| q.id).unwrap_or(MAIN_QUEUE);
            let folder = d.get_folder().trim().to_string();
            let use_text = d.get_use_text();
            let mut added = 0;
            for it in items {
                if ui.ctx.mgr.find_url(&it.url).is_some() {
                    continue;
                }
                let mut h = headers.clone();
                if h.referer.is_none() && !it.page.is_empty() {
                    h.referer = Some(it.page.clone());
                }
                ui.ctx.mgr.add(NewDownload {
                    url: it.url.clone(),
                    save_dir: (!folder.is_empty()).then(|| PathBuf::from(&folder)),
                    headers: h,
                    queue: Some(queue),
                    description: (use_text && !it.text.is_empty()).then(|| it.text.clone()),
                    start: false,
                    ..Default::default()
                });
                added += 1;
            }
            if d.get_start_now() {
                ui.ctx.mgr.start_queue(queue);
            }
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_view(format!("queue:{queue}").into());
            }
            d.set_message(SharedString::new());
            d.set_checked_text(tf("{} file(s) added to the queue", &[&added]).into());
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        d.on_close_window(move || {
            if let Some(c) = ui.grab_state.borrow_mut().cancel.take() {
                c.cancel();
            }
            ui.grab_state.borrow_mut().exploring = false;
            if let Some(d) = ui.grabber.upgrade() {
                let _ = d.hide();
            }
        });
    }
}
