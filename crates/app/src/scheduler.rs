//! The Scheduler window: every queue with its schedule, its files (in
//! download order) and its options.

use crate::open_window;
use crate::*;
use swiftget_engine::config::{parse_hhmm, QueueDef};
use crate::i18n::{t, tf};

#[derive(Default)]
pub(crate) struct SchedState {
    /// queue ids in the window's list
    queue_ids: Vec<u64>,
    current: Option<u64>,
    /// download ids of the "Files in the queue" list
    file_ids: Vec<u64>,
}

fn queue_rows(ui: &Ui) -> Vec<QueueRow> {
    let s = ui.ctx.mgr.settings();
    let views = ui.ctx.mgr.snapshot();
    s.queues
        .iter()
        .map(|q| QueueRow {
            id: q.id as i32,
            name: q.name.clone().into(),
            count: views.iter().filter(|v| v.queue == Some(q.id) && v.status != Status::Completed).count() as i32,
            running: ui.ctx.mgr.queue_running(q.id),
        })
        .collect()
}

/// Open the window on a queue (None = the one selected in the sidebar).
pub(crate) fn open(ui: &Ui, queue: Option<u64>) {
    let Some(d) = ui.sched.upgrade() else { return };
    let s = ui.ctx.mgr.settings();
    let wanted = queue
        .or_else(|| ui.ctx.main.upgrade().map(|m| m.get_current_queue() as u64))
        .filter(|q| s.queue(*q).is_some())
        .unwrap_or(MAIN_QUEUE);
    fill_list(ui, &d);
    select(ui, &d, wanted);
    d.set_message(SharedString::new());
    open_window!(d);
    d.window().set_minimized(false);
}

fn fill_list(ui: &Ui, d: &SchedulerDialog) {
    let rows = queue_rows(ui);
    ui.sched_state.borrow_mut().queue_ids = rows.iter().map(|r| r.id as u64).collect();
    d.set_queues(ModelRc::new(VecModel::from(rows)));
}

fn select(ui: &Ui, d: &SchedulerDialog, id: u64) {
    let s = ui.ctx.mgr.settings();
    let Some(q) = s.queue(id).cloned() else { return };
    let index = ui.sched_state.borrow().queue_ids.iter().position(|x| *x == id).unwrap_or(0);
    ui.sched_state.borrow_mut().current = Some(id);
    d.set_current(index as i32);
    d.set_is_main(id == MAIN_QUEUE);
    d.set_name(q.name.clone().into());
    let sc = &q.schedule;
    d.set_start_enabled(sc.start_enabled);
    d.set_start_time(sc.start_time.clone().into());
    d.set_once(sc.once);
    d.set_once_date(if sc.once_date.is_empty() {
        chrono::Local::now().format("%Y-%m-%d").to_string().into()
    } else {
        sc.once_date.clone().into()
    });
    d.set_d0(sc.days[0]);
    d.set_d1(sc.days[1]);
    d.set_d2(sc.days[2]);
    d.set_d3(sc.days[3]);
    d.set_d4(sc.days[4]);
    d.set_d5(sc.days[5]);
    d.set_d6(sc.days[6]);
    d.set_on_startup(sc.on_startup);
    d.set_stop_enabled(sc.stop_enabled);
    d.set_stop_time(sc.stop_time.clone().into());
    d.set_parallel(q.max_parallel.clamp(1, 32) as i32);
    d.set_retries(q.retries.min(50) as i32);
    d.set_folder(q.folder.as_ref().map(|p| p.display().to_string()).unwrap_or_default().into());
    d.set_when_done(q.after_done.index());
    d.set_open_when_done(q.open_when_done);
    d.set_file_current(-1);
    fill_files(ui, d);
}

fn fill_files(ui: &Ui, d: &SchedulerDialog) {
    let Some(q) = ui.sched_state.borrow().current else { return };
    let files = ui.ctx.mgr.queue_files(q);
    let rows: Vec<PairRow> = files
        .iter()
        .map(|v| PairRow {
            a: v.file_name.clone().into(),
            b: v.total_size.map(fmt_size).unwrap_or_default().into(),
            c: match v.status {
                Status::Downloading | Status::Paused if v.total_size.is_some() => {
                    format!("{:.1}%  {}", v.progress() * 100.0, if v.status == Status::Paused { "" } else { "▶" })
                }
                other => t(other.label()),
            }
            .into(),
        })
        .collect();
    let ids: Vec<u64> = files.iter().map(|v| v.id).collect();
    let changed = ui.sched_state.borrow().file_ids != ids || d.get_files().row_count() != rows.len();
    ui.sched_state.borrow_mut().file_ids = ids;
    if changed {
        d.set_files(ModelRc::new(VecModel::from(rows)));
    } else if let Some(m) = d.get_files().as_any().downcast_ref::<VecModel<PairRow>>() {
        update_model(m, rows);
    } else {
        d.set_files(ModelRc::new(VecModel::from(rows)));
    }
    d.set_running(ui.ctx.mgr.queue_running(q));
}

/// Keep the running marks and the files list current while the window is open.
pub(crate) fn poll(ui: &Ui) {
    let Some(d) = ui.sched.upgrade() else { return };
    if !d.window().is_visible() {
        return;
    }
    let rows = queue_rows(ui);
    if let Some(m) = d.get_queues().as_any().downcast_ref::<VecModel<QueueRow>>() {
        if m.row_count() == rows.len() {
            update_model(m, rows);
        }
    }
    fill_files(ui, &d);
}

/// The window's fields -> the queue definition.
fn read(d: &SchedulerDialog, mut q: QueueDef) -> Result<QueueDef, String> {
    let name = d.get_name().trim().to_string();
    if name.is_empty() {
        return Err(t("Give the queue a name"));
    }
    let start = d.get_start_time().trim().to_string();
    let stop = d.get_stop_time().trim().to_string();
    if d.get_start_enabled() && parse_hhmm(&start).is_none() {
        return Err(t("The start time must look like 02:30"));
    }
    if d.get_stop_enabled() && parse_hhmm(&stop).is_none() {
        return Err(t("The stop time must look like 08:00"));
    }
    let date = d.get_once_date().trim().to_string();
    if d.get_start_enabled() && d.get_once() && chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
        return Err(t("The date must look like 2026-12-31"));
    }
    q.name = name;
    q.schedule.start_enabled = d.get_start_enabled();
    q.schedule.start_time = start;
    q.schedule.once = d.get_once();
    q.schedule.once_date = date;
    q.schedule.days = [d.get_d0(), d.get_d1(), d.get_d2(), d.get_d3(), d.get_d4(), d.get_d5(), d.get_d6()];
    q.schedule.on_startup = d.get_on_startup();
    q.schedule.stop_enabled = d.get_stop_enabled();
    q.schedule.stop_time = stop;
    q.max_parallel = d.get_parallel().clamp(1, 32) as u32;
    q.retries = d.get_retries().clamp(0, 50) as u32;
    let folder = d.get_folder().trim().to_string();
    q.folder = (!folder.is_empty()).then(|| PathBuf::from(folder));
    q.after_done = AfterDone::from_index(d.get_when_done());
    q.open_when_done = d.get_open_when_done();
    Ok(q)
}

fn apply(ui: &Ui) -> bool {
    let Some(d) = ui.sched.upgrade() else { return false };
    let Some(id) = ui.sched_state.borrow().current else { return false };
    let Some(q) = ui.ctx.mgr.settings().queue(id).cloned() else { return false };
    match read(&d, q).map_err(anyhow::Error::msg).and_then(|q| ui.ctx.mgr.update_queue(q)) {
        Ok(()) => {
            d.set_message(SharedString::new());
            fill_list(ui, &d);
            refresh(ui);
            true
        }
        Err(e) => {
            d.set_message(format!("{e:#}").into());
            false
        }
    }
}

pub(crate) fn new_queue(ui: &Ui) {
    let s = ui.ctx.mgr.settings();
    let mut n = s.queues.len();
    let name = loop {
        let candidate = format!("Queue {n}");
        if !s.queues.iter().any(|q| q.name == candidate) {
            break candidate;
        }
        n += 1;
    };
    match ui.ctx.mgr.add_queue(&name, 1, None) {
        Ok(id) => {
            open(ui, Some(id));
            if let Some(m) = ui.ctx.main.upgrade() {
                m.set_view(format!("queue:{id}").into());
            }
            refresh(ui);
        }
        Err(e) => message(&tf("Could not create the queue: {}", &[&format!("{e:#}")])),
    }
}

pub(crate) fn delete_queue(ui: &Ui, id: u64) {
    if id == MAIN_QUEUE {
        message(&t("The main download queue can't be deleted."));
        return;
    }
    let name = ui.ctx.mgr.settings().queue(id).map(|q| q.name.clone()).unwrap_or_default();
    if !ask(&tf("Delete the queue “{}”?\nIts files move to the main download queue.", &[&name])) {
        return;
    }
    match ui.ctx.mgr.delete_queue(id) {
        Ok(()) => {
            if let Some(m) = ui.ctx.main.upgrade() {
                if m.get_view() == format!("queue:{id}").as_str() {
                    m.set_view("all".into());
                }
            }
            if let Some(d) = ui.sched.upgrade() {
                if d.window().is_visible() {
                    fill_list(ui, &d);
                    select(ui, &d, MAIN_QUEUE);
                }
            }
            refresh(ui);
        }
        Err(e) => message(&format!("{e:#}")),
    }
}

pub(crate) fn wire(ui: &Ui) {
    let Some(d) = ui.sched.upgrade() else { return };
    {
        let ui = ui.clone();
        d.on_pick_queue(move |i| {
            let Some(d) = ui.sched.upgrade() else { return };
            let id = ui.sched_state.borrow().queue_ids.get(i.max(0) as usize).copied();
            if let Some(id) = id {
                select(&ui, &d, id);
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_new_queue(move || new_queue(&ui));
    }
    {
        let ui = ui.clone();
        d.on_delete_queue(move || {
            if let Some(id) = ui.sched_state.borrow().current {
                delete_queue(&ui, id);
            }
        });
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
    let move_file = |ui: &Ui, delta: i32| {
        let Some(d) = ui.sched.upgrade() else { return };
        let i = d.get_file_current();
        let id = ui.sched_state.borrow().file_ids.get(i.max(0) as usize).copied();
        if let (true, Some(id)) = (i >= 0, id) {
            ui.ctx.mgr.move_in_queue(id, delta);
            fill_files(ui, &d);
            d.set_file_current(i + delta);
        }
    };
    {
        let ui = ui.clone();
        d.on_file_up(move || move_file(&ui, -1));
    }
    {
        let ui = ui.clone();
        d.on_file_down(move || move_file(&ui, 1));
    }
    {
        let ui = ui.clone();
        d.on_file_remove(move || {
            let Some(d) = ui.sched.upgrade() else { return };
            let i = d.get_file_current();
            let id = ui.sched_state.borrow().file_ids.get(i.max(0) as usize).copied();
            if let (true, Some(id)) = (i >= 0, id) {
                ui.ctx.mgr.set_queue(id, None);
                d.set_file_current(-1);
                fill_files(&ui, &d);
                refresh(&ui);
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_start_now(move || {
            if apply(&ui) {
                if let Some(id) = ui.sched_state.borrow().current {
                    ui.ctx.mgr.start_queue(id);
                }
                refresh(&ui);
            }
        });
    }
    {
        let ui = ui.clone();
        d.on_stop_queue(move || {
            if let Some(id) = ui.sched_state.borrow().current {
                ui.ctx.mgr.stop_queue(id);
            }
            refresh(&ui);
        });
    }
    {
        let ui = ui.clone();
        d.on_apply(move || {
            apply(&ui);
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
