//! "Download a playlist": every video of a playlist in one go, in
//! the quality you choose, into its own queue and folder.

use crate::open_window;
use crate::*;

use crate::i18n::{t, tf};

const QUALITIES: [&str; 9] = ["best", "2160", "1440", "1080", "720", "480", "360", "audio", "mp3"];

pub(crate) fn open(ui: &Ui) {
    let Some(d) = ui.playlist.upgrade() else { return };
    if !d.window().is_visible() {
        // a playlist address in the clipboard is the likely one
        let clip = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()).unwrap_or_default();
        let clip = clip.trim();
        if clip.contains("list=") || clip.contains("/playlist") || clip.contains("/@") {
            d.set_url(clip.into());
        }
        d.set_message(SharedString::new());
    }
    open_window!(d);
    d.window().set_minimized(false);
}

/// The browser's "Download all" on a playlist page: the window with that
/// list, already looking up its videos.
pub(crate) fn open_url(ui: &Ui, url: &str) {
    let Some(d) = ui.playlist.upgrade() else { return };
    d.set_url(url.into());
    d.set_message(SharedString::new());
    open_window!(d);
    d.window().set_minimized(false);
    if !d.get_busy() {
        d.invoke_fetch();
    }
}

fn fmt_len(secs: u64) -> String {
    if secs == 0 {
        String::new()
    } else if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

fn checked_text(rows: &VecModel<LinkRow>) -> String {
    let n = (0..rows.row_count()).filter_map(|i| rows.row_data(i)).filter(|r| r.checked).count();
    tf("{} of {} videos checked", &[&n, &rows.row_count()])
}

pub(crate) fn wire(ui: &Ui) {
    let Some(d) = ui.playlist.upgrade() else { return };
    let rows = ui.playlist_rows.clone();
    d.set_entries(ModelRc::from(rows.clone()));
    {
        let (w, mgr) = (d.as_weak(), ui.ctx.mgr.clone());
        d.on_fetch(move || {
            let Some(d) = w.upgrade() else { return };
            let url = d.get_url().trim().to_string();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                d.set_message(t("Paste the playlist address (https://…/playlist?list=…)").into());
                return;
            }
            d.set_message(SharedString::new());
            d.set_busy(true);
            d.set_status(t("Getting the list of videos… (a few seconds)").into());
            let w2 = d.as_weak();
            let base = mgr.settings().dir_for("Video");
            std::thread::spawn(move || {
                let result = swiftget_engine::ytdlp::playlist(&url);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(d) = w2.upgrade() else { return };
                    d.set_busy(false);
                    match result {
                        Ok(p) if p.entries.is_empty() => {
                            d.set_status(t("No videos were found at this address.").into());
                        }
                        Ok(p) => {
                            let list: Vec<LinkRow> = p
                                .entries
                                .iter()
                                .enumerate()
                                .map(|(i, e)| LinkRow {
                                    checked: true,
                                    name: e.title.clone().into(),
                                    kind: "video".into(),
                                    ext: fmt_len(e.duration).into(),
                                    text: format!("{}", i + 1).into(),
                                    url: e.url.clone().into(),
                                })
                                .collect();
                            if let Some(m) = d.get_entries().as_any().downcast_ref::<VecModel<LinkRow>>() {
                                m.set_vec(list);
                                d.set_checked_text(checked_text(m).into());
                            }
                            d.set_playlist_title(p.title.clone().into());
                            d.set_status(tf("{} videos", &[&p.entries.len()]).into());
                            let folder = base.join(filename::sanitize(&p.title));
                            d.set_folder(folder.display().to_string().into());
                        }
                        Err(e) => d.set_status(tf("Could not get the list: {}", &[&crate::err_text(&format!("{e:#}"))]).into()),
                    }
                });
            });
        });
    }
    {
        let (w, rows) = (d.as_weak(), rows.clone());
        d.on_toggle(move |i| {
            if let Some(mut r) = rows.row_data(i.max(0) as usize) {
                r.checked = !r.checked;
                rows.set_row_data(i as usize, r);
            }
            if let Some(d) = w.upgrade() {
                d.set_checked_text(checked_text(&rows).into());
            }
        });
    }
    {
        let (w, rows) = (d.as_weak(), rows.clone());
        d.on_check_all(move |on| {
            for i in 0..rows.row_count() {
                if let Some(mut r) = rows.row_data(i) {
                    r.checked = on;
                    rows.set_row_data(i, r);
                }
            }
            if let Some(d) = w.upgrade() {
                d.set_checked_text(checked_text(&rows).into());
            }
        });
    }
    {
        let w = d.as_weak();
        d.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                d.set_folder(dir.display().to_string().into());
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_download(move || {
            let Some(d) = ui.playlist.upgrade() else { return };
            let chosen: Vec<LinkRow> =
                (0..ui.playlist_rows.row_count()).filter_map(|i| ui.playlist_rows.row_data(i)).filter(|r| r.checked).collect();
            if chosen.is_empty() {
                d.set_message(t("Check the videos you want first").into());
                return;
            }
            let folder = d.get_folder().trim().to_string();
            if folder.is_empty() {
                d.set_message(t("Choose where to save the videos").into());
                return;
            }
            let folder = PathBuf::from(folder);
            let quality = QUALITIES[d.get_quality().clamp(0, QUALITIES.len() as i32 - 1) as usize].to_string();
            let ext = match quality.as_str() { "audio" => "m4a", "mp3" => "mp3", _ => "mp4" };
            let title = d.get_playlist_title().to_string();
            // its own queue: N videos at a time, all into the folder
            let name = if title.trim().is_empty() { "Playlist".to_string() } else { title.trim().chars().take(60).collect() };
            let queue = match ui.ctx.mgr.add_queue(&name, d.get_parallel().clamp(1, 8) as u32, Some(folder.clone())) {
                Ok(q) => q,
                Err(e) => {
                    d.set_message(format!("{e:#}").into());
                    return;
                }
            };
            let width = chosen.len().to_string().len().max(2);
            for r in &chosen {
                let mut file = filename::sanitize(&r.name);
                if d.get_numbered() {
                    let n: usize = r.text.parse().unwrap_or(0);
                    file = format!("{n:0width$} - {file}");
                }
                ui.ctx.mgr.add(NewDownload {
                    url: r.url.to_string(),
                    file_name: Some(format!("{file}.{ext}")),
                    save_dir: Some(folder.clone()),
                    source_page: Some(r.url.to_string()),
                    pick_quality: Some(quality.clone()),
                    queue: Some(queue),
                    description: Some(title.clone()).filter(|t| !t.is_empty()),
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
            let _ = std::fs::create_dir_all(&folder);
            let _ = d.hide();
            refresh(&ui);
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
