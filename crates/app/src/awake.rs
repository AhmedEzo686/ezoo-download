//! Scheduled queues and sleep: while a queue is waiting for its start time
//! the computer is kept awake, so the download really starts at night.
//!
//! It is held only when the start is near (within half a day) and the queue
//! is not running: an always-on inhibitor would keep a laptop awake for
//! nothing.

use chrono::{Datelike, Local, NaiveDate, NaiveTime, Timelike};

/// How near a start must be before the computer is kept awake.
const NEAR: i64 = 12 * 60 * 60;

/// Look at the queues and hold or release the computer.
pub fn update(ui: &crate::Ui) {
    let mgr = &ui.ctx.mgr;
    let now = Local::now();
    let pending = mgr.settings().queues.iter().any(|q| {
        let s = &q.schedule;
        if !s.start_enabled || mgr.queue_running(q.id) {
            return false;
        }
        let Some(at) = next_start(s, now) else { return false };
        let wait = (at - now).num_seconds();
        wait >= 0 && wait <= NEAR
    });
    keep(pending);
}

fn next_start(s: &swiftget_engine::config::QueueSchedule, now: chrono::DateTime<Local>) -> Option<chrono::DateTime<Local>> {
    let time = NaiveTime::parse_from_str(s.start_time.trim(), "%H:%M").ok()?;
    if s.once {
        let date = NaiveDate::parse_from_str(s.once_date.trim(), "%Y-%m-%d").ok()?;
        return Some(date.and_time(time).and_local_timezone(Local).single()?);
    }
    for add in 0..=7 {
        let day = now.date_naive() + chrono::Duration::days(add);
        // Monday .. Sunday in the settings, Monday = 0
        if !s.days[day.weekday().num_days_from_monday() as usize] {
            continue;
        }
        let at = day.and_time(time).and_local_timezone(Local).single()?;
        if at > now {
            return Some(at);
        }
    }
    None
}

/// On: hold the computer. Off: let it sleep again.
fn keep(on: bool) {
    let mut held = HELD.lock().unwrap_or_else(|p| p.into_inner());
    if on == held.is_some() {
        return;
    }
    if on {
        *held = hold();
    } else {
        *held = None; // dropping releases it
    }
}

static HELD: std::sync::Mutex<Option<Hold>> = std::sync::Mutex::new(None);

/// Linux: logind's inhibitor, through systemd-inhibit. A child that sleeps
/// forever; killing it releases the lock. Missing systemd-inhibit (a non
/// systemd system) just means nothing is held.
#[cfg(target_os = "linux")]
struct Hold {
    child: std::process::Child,
}

#[cfg(target_os = "linux")]
impl Drop for Hold {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(target_os = "linux")]
fn hold() -> Option<Hold> {
    let child = std::process::Command::new("systemd-inhibit")
        .args([
            "--what=sleep",
            "--who=Ezoo Download",
            "--why=Waiting for a scheduled download",
            "--mode=block",
            "sleep",
            "infinity",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    Some(Hold { child })
}

/// Windows: the power request the system keeps while it is set.
#[cfg(windows)]
struct Hold;

#[cfg(windows)]
impl Drop for Hold {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Power::SetThreadExecutionState;
        use windows_sys::Win32::System::Power::ES_CONTINUOUS;
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

#[cfg(windows)]
fn hold() -> Option<Hold> {
    use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};
    let ok = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
    (ok != 0).then_some(Hold)
}

#[cfg(not(any(windows, target_os = "linux")))]
struct Hold;

#[cfg(not(any(windows, target_os = "linux")))]
fn hold() -> Option<Hold> {
    None
}
