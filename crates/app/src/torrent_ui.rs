//! "Add a torrent": a magnet link or a .torrent file, the files it shares
//! (each can be left out), the folder, and off it goes like any download.

use crate::open_window;
use crate::*;

use crate::i18n::{t, tf};
use swiftget_engine::torrent;

thread_local! {
    /// opens the window from places that only have the shared context
    static OPENER: std::cell::RefCell<Option<Ui>> = const { std::cell::RefCell::new(None) };
}

/// Open the torrent window with this magnet link / .torrent file.
pub(crate) fn open_source(source: &str) {
    if let Some(ui) = OPENER.with(|o| o.borrow().clone()) {
        open(&ui, Some(source));
    }
}

pub(crate) fn open(ui: &Ui, source: Option<&str>) {
    let Some(d) = ui.torrent.upgrade() else { return };
    match source {
        Some(s) => d.set_source(s.into()),
        None if !d.window().is_visible() => {
            // a magnet link in the clipboard is the likely one
            let clip = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()).unwrap_or_default();
            d.set_source(if torrent::is_magnet(&clip) { clip.trim().into() } else { SharedString::new() });
        }
        None => {}
    }
    d.invoke_load();
    open_window!(d);
    d.window().set_minimized(false);
}

fn checked_text(rows: &VecModel<LinkRow>) -> String {
    let n = (0..rows.row_count()).filter_map(|i| rows.row_data(i)).filter(|r| r.checked).count();
    tf("{} of {} files checked", &[&n, &rows.row_count()])
}

/// "1,3,4": the torrent's numbers of the checked files (kept in each row's
/// `text`), `None` when all are checked.
fn selection(rows: &VecModel<LinkRow>) -> Option<String> {
    let all: Vec<LinkRow> = (0..rows.row_count()).filter_map(|i| rows.row_data(i)).collect();
    if all.iter().all(|r| r.checked) {
        return None;
    }
    Some(all.iter().filter(|r| r.checked).map(|r| r.text.to_string()).collect::<Vec<_>>().join(","))
}

pub(crate) fn wire(ui: &Ui) {
    OPENER.with(|o| *o.borrow_mut() = Some(ui.clone()));
    let Some(d) = ui.torrent.upgrade() else { return };
    let rows = ui.torrent_rows.clone();
    d.set_files(ModelRc::from(rows.clone()));
    {
        let (w, mgr, rows) = (d.as_weak(), ui.ctx.mgr.clone(), rows.clone());
        d.on_load(move || {
            let Some(d) = w.upgrade() else { return };
            let src = d.get_source().trim().trim_matches('"').to_string();
            rows.set_vec(Vec::new());
            d.set_message(SharedString::new());
            d.set_magnet(false);
            let settings = mgr.settings();
            if torrent::is_magnet(&src) {
                d.set_magnet(true);
                d.set_name(torrent::magnet_name(&src).unwrap_or_else(|| t("Magnet link")).into());
                d.set_info(t("The file list comes from the other computers when it starts").into());
                d.set_checked_text(SharedString::new());
                if d.get_folder().is_empty() {
                    d.set_folder(settings.dir_for("General").display().to_string().into());
                }
                return;
            }
            d.set_name(SharedString::new());
            d.set_info(SharedString::new());
            d.set_checked_text(SharedString::new());
            if src.is_empty() {
                return;
            }
            if !src.to_ascii_lowercase().ends_with(".torrent") {
                if src.len() > 8 {
                    d.set_message(t("This is not a magnet link or a .torrent file").into());
                }
                return;
            }
            let info = match std::fs::read(&src).map_err(anyhow::Error::from).and_then(|b| torrent::parse_torrent(&b)) {
                Ok(i) => i,
                Err(e) => {
                    d.set_message(tf("This .torrent file can't be read: {}", &[&e]).into());
                    return;
                }
            };
            d.set_name(info.name.clone().into());
            d.set_info(tf("{} in {} files", &[&fmt_size(info.total), &info.files.len()]).into());
            rows.set_vec(
                info.files
                    .iter()
                    .map(|f| LinkRow {
                        checked: true,
                        name: f.path.clone().into(),
                        kind: kind_for(&f.path).into(),
                        ext: fmt_size(f.size).into(),
                        text: f.index.to_string().into(),
                        url: SharedString::new(),
                    })
                    .collect::<Vec<_>>(),
            );
            d.set_checked_text(checked_text(&rows).into());
            // the folder of what it mostly is (a film: Video)
            if let Some(big) = info.files.iter().max_by_key(|f| f.size) {
                d.set_folder(settings.dir_for(&settings.category_of(&big.path, "")).display().to_string().into());
            }
        });
    }
    {
        let w = d.as_weak();
        d.on_pick_file(move || {
            let Some(d) = w.upgrade() else { return };
            // on top of this window, which waits for it
            d.set_front(false);
            if let Some(p) = crate::winfx::on_top_of(rfd::FileDialog::new(), d.window()).add_filter("Torrent", &["torrent"]).pick_file() {
                d.set_source(p.display().to_string().into());
                d.invoke_load();
            }
        });
    }
    {
        let (w, rows) = (d.as_weak(), rows.clone());
        d.on_toggle(move |i| {
            let Some(d) = w.upgrade() else { return };
            if let Some(mut r) = rows.row_data(i as usize) {
                r.checked = !r.checked;
                rows.set_row_data(i as usize, r);
            }
            d.set_checked_text(checked_text(&rows).into());
        });
    }
    {
        let (w, rows) = (d.as_weak(), rows.clone());
        d.on_check_all(move |on| {
            let Some(d) = w.upgrade() else { return };
            for i in 0..rows.row_count() {
                if let Some(mut r) = rows.row_data(i) {
                    r.checked = on;
                    rows.set_row_data(i, r);
                }
            }
            d.set_checked_text(checked_text(&rows).into());
        });
    }
    {
        let w = d.as_weak();
        d.on_browse(move || {
            let Some(d) = w.upgrade() else { return };
            d.set_front(false);
            if let Some(dir) = crate::winfx::on_top_of(rfd::FileDialog::new(), d.window()).set_directory(d.get_folder().as_str()).pick_folder() {
                d.set_folder(dir.display().to_string().into());
            }
        });
    }
    {
        let (w, mgr, rows) = (d.as_weak(), ui.ctx.mgr.clone(), rows.clone());
        d.on_download(move || {
            let Some(d) = w.upgrade() else { return };
            let src = d.get_source().trim().trim_matches('"').to_string();
            if !torrent::is_torrent_source(&src) {
                d.set_message(t("This is not a magnet link or a .torrent file").into());
                return;
            }
            let select = selection(&rows);
            if select.as_deref() == Some("") {
                d.set_message(t("Check at least one file").into());
                return;
            }
            let folder = d.get_folder().trim().to_string();
            if folder.is_empty() {
                d.set_message(t("Choose where to save it").into());
                return;
            }
            let start = d.get_start_now();
            let id = mgr.add(NewDownload {
                url: src,
                save_dir: Some(PathBuf::from(folder)),
                start,
                torrent_select: select,
                ..Default::default()
            });
            d.set_source(SharedString::new());
            rows.set_vec(Vec::new());
            let _ = d.hide();
            // its progress window, like any download that starts
            if start {
                if let Some(ui) = OPENER.with(|o| o.borrow().clone()) {
                    crate::show_progress_for_start(&ui, id);
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
    use super::*;

    fn row(checked: bool, index: usize) -> LinkRow {
        LinkRow { checked, text: index.to_string().into(), ..Default::default() }
    }

    #[test]
    fn chosen_files() {
        // the torrent's own numbers (a padding file was number 2)
        let rows = VecModel::from(vec![row(true, 1), row(false, 3), row(true, 4)]);
        assert_eq!(selection(&rows).as_deref(), Some("1,4"));
        let all = VecModel::from(vec![row(true, 1), row(true, 2)]);
        assert_eq!(selection(&all), None);
        let none = VecModel::from(vec![row(false, 1)]);
        assert_eq!(selection(&none).as_deref(), Some(""));
    }
}
