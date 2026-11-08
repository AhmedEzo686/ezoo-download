//! Multi-connection download of a normal HTTP file.
//!
//! Every connection ("worker") asks the segment table for work, downloads its
//! byte range with an HTTP Range request and writes straight into the right
//! offset of the `.sgpart` file. When a worker finishes it steals half of the
//! biggest remaining segment, so all connections stay busy until the end.

use crate::http::SendOrdered;
use crate::fileio::{make_sparse, set_hidden, write_at};
use crate::http::{http_status_of, is_network_error, reconnect_delay, request, ConnGuard, HttpStatus, NetDrop, Progress, Shared};
use crate::model::RequestHeaders;
use crate::segments::{self, Segment};
use anyhow::{anyhow, bail};
use reqwest::header::RANGE;
use reqwest::StatusCode;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

/// How long we wait for the next chunk before treating the connection as dead.
const STALL_TIMEOUT: Duration = Duration::from_secs(20);
/// How long the other connections are watched before a refused one (403)
/// is taken for "one connection too many".
#[cfg(not(test))]
const OTHERS_WATCH: Duration = Duration::from_secs(2);
#[cfg(test)]
const OTHERS_WATCH: Duration = Duration::from_millis(400);

/// How long a request waits for the server's answer before it is made again.
const HEADERS_TIMEOUT: Duration = Duration::from_secs(20);

/// A connection much slower than the others is dropped and made again (the
/// server often gives a new one full speed): checked over this long...
const SLOW_WINDOW: Duration = Duration::from_secs(8);
/// ...when it brings less than this...
const SLOW_BYTES_PER_SEC: u64 = 48 * 1024;
/// ...and less than this part of what one connection brings on average.
const SLOW_SHARE: u64 = 6;

/// This connection is more than the user wants now: it stops (its part is
/// left to the others).
#[derive(Debug)]
struct Leave;
impl std::fmt::Display for Leave {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("connection not needed")
    }
}
impl std::error::Error for Leave {}

/// How many connections run and how many were asked to stop.
#[derive(Default)]
struct Crew {
    running: u32,
    leaving: u32,
    /// connections the server dropped lately (when, and the bytes of the
    /// whole download then)
    drops: std::collections::VecDeque<(std::time::Instant, u64)>,
    /// the best a connection brought on average (bytes a second): the last
    /// one left alone is held to it
    best: u64,
}

/// At the end of a download, parts are shared down to this size (the last
/// piece is not left to one slow connection).
const TAIL_SPLIT: u64 = 64 * 1024;

/// After this long without the server refusing (seconds), one connection more
/// is tried again (up to as many as the download began with).
const CLIMB_AFTER: u64 = 60;

#[derive(Clone)]
pub struct HttpJob {
    pub url: String,
    pub headers: RequestHeaders,
    pub user_agent: String,
    pub total_size: Option<u64>,
    pub resumable: bool,
    pub part_path: PathBuf,
    pub connections: u32,
    pub min_split: u64,
    pub retries: u32,
    /// Bytes from earlier stages of the same download (Dash: the video part
    /// when this job is the audio part), added to the progress counter.
    pub progress_base: u64,
    /// Ask for at most this many bytes per HTTP request. YouTube slows a
    /// connection down to playback speed when one request asks for too much,
    /// so for its servers we fetch each part in 10 MB requests.
    pub max_request: Option<u64>,
    /// this download's own speed limit (shared with its other parts)
    pub limiter: Arc<crate::limiter::SpeedLimiter>,
}

/// Request size limit for servers known to throttle big range requests.
pub fn max_request_for(url: &str) -> Option<u64> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    if host.ends_with("googlevideo.com") {
        Some(10 * 1024 * 1024)
    } else {
        None
    }
}

pub type SegTable = Arc<Mutex<Vec<Segment>>>;

#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}
impl std::error::Error for Cancelled {}

pub async fn run(
    shared: Arc<Shared>,
    job: HttpJob,
    segs: SegTable,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let part = job.part_path.clone();
    let r = run_hidden(shared, job, segs, progress, cancel).await;
    // however it ended: the temporary file is shown again
    set_hidden(&part, false);
    r
}

async fn run_hidden(
    shared: Arc<Shared>,
    job: HttpJob,
    segs: SegTable,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    if let Some(dir) = job.part_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let segmented = job.resumable && job.total_size.is_some();
    let mut opts = OpenOptions::new();
    opts.create(true).read(true).write(true).truncate(false);
    // nobody can delete or move the file while it downloads (Windows let
    // another program, a cleaner or an antivirus, remove it: the download
    // went on into nothing and the file was gone at 100 %); reading it
    // ("Play while downloading") still works
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_WRITE: u32 = 2;
        opts.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    let file = opts.open(&job.part_path)?;
    // while it downloads, nobody takes it for the file (it is not yet)
    set_hidden(&job.part_path, true);

    {
        let mut table = segs.lock().unwrap();
        if segmented {
            let total = job.total_size.unwrap_or(0);
            let fresh = table.is_empty() || file.metadata()?.len() == 0 && segments::downloaded(&table) > 0;
            if fresh {
                *table = segments::plan(total, job.connections, job.min_split);
                file.set_len(0)?;
            }
            if file.metadata()?.len() != total {
                make_sparse(&file);
                file.set_len(total)?;
            }
        } else {
            // no ranges: always start over with one connection
            file.set_len(0)?;
            *table = vec![Segment::new(0, job.total_size.unwrap_or(u64::MAX))];
        }
        for s in table.iter_mut() {
            s.active = false;
            // what is there now was made sure of (the last download ended
            // with it on the disk)
            s.on_disk = Some(s.pos);
        }
        progress
            .bytes
            .store(job.progress_base + segments::downloaded(&table), Ordering::Relaxed);
    }

    let file = Arc::new(file);
    let child = cancel.child_token();
    let crew = Arc::new(Mutex::new(Crew::default()));
    let wanted = {
        let (progress, job_conns) = (progress.clone(), job.connections.max(1));
        move || {
            if !segmented {
                return 1;
            }
            match progress.want_connections.load(Ordering::Relaxed) {
                0 => job_conns,
                n => n.clamp(1, 64),
            }
        }
    };
    let mut set = JoinSet::new();
    let spawn = |set: &mut JoinSet<anyhow::Result<()>>| {
        crew.lock().unwrap().running += 1;
        let ctx = Worker {
            shared: shared.clone(),
            job: job.clone(),
            segs: segs.clone(),
            progress: progress.clone(),
            file: file.clone(),
            cancel: child.clone(),
            segmented,
            crew: crew.clone(),
            wanted: Arc::new(wanted.clone()),
            retry_after: std::sync::atomic::AtomicU64::new(0),
        };
        set.spawn(ctx.run());
    };
    for _ in 0..wanted() {
        spawn(&mut set);
    }

    let mut first_error: Option<anyhow::Error> = None;
    // every few seconds what was written is made sure to be on the disk
    // (and the list may save that far)
    let mut to_disk = tokio::time::interval(Duration::from_secs(4));
    to_disk.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    to_disk.tick().await;
    let mut look = tokio::time::interval(Duration::from_millis(500));
    look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // what one connection brings at best (measured every 2 s over the whole
    // download)
    let mut sample = (std::time::Instant::now(), progress.bytes.load(Ordering::Relaxed));
    loop {
        tokio::select! {
            res = set.join_next() => {
                let Some(res) = res else {
                    // every connection is done: more work left (fewer were
                    // wanted for a moment, then more again)? then on
                    if first_error.is_none() && !cancel.is_cancelled() && segmented && wanted() > 0 {
                        let more = !segments::all_finished(&segs.lock().unwrap());
                        if more {
                            spawn(&mut set);
                            continue;
                        }
                    }
                    break;
                };
                let res = res.map_err(|e| anyhow!("worker crashed: {e}")).and_then(|r| r);
                if let Err(e) = res {
                    if first_error.is_none() && !cancel.is_cancelled() {
                        first_error = Some(e);
                    }
                    child.cancel();
                }
            }
            _ = to_disk.tick() => {
                let seen: Vec<u64> = segs.lock().unwrap().iter().map(|s| s.pos).collect();
                let f = file.clone();
                let synced = tokio::task::spawn_blocking(move || f.sync_data().is_ok()).await.unwrap_or(false);
                if synced {
                    let mut table = segs.lock().unwrap();
                    for (s, pos) in table.iter_mut().zip(seen) {
                        s.on_disk = Some(pos.min(s.pos));
                    }
                }
            }
            _ = look.tick() => {
                let spent = sample.0.elapsed();
                if spent >= Duration::from_secs(2) {
                    let now = progress.bytes.load(Ordering::Relaxed);
                    let conns = progress.connections.load(Ordering::Relaxed).max(1) as u64;
                    let per = now.saturating_sub(sample.1) * 1000 / (spent.as_millis().max(1) as u64) / conns;
                    let mut c = crew.lock().unwrap();
                    c.best = c.best.max(per);
                    drop(c);
                    sample = (std::time::Instant::now(), now);
                }
                // the server made it take fewer connections: a minute later
                // without that, one more is tried (it lowers again if not)
                if segmented {
                    let at = progress.lowered_at.load(Ordering::Relaxed);
                    let want = progress.want_connections.load(Ordering::Relaxed);
                    let began = job.connections.max(1);
                    if at > 0 && want > 0 && want < began && now_secs().saturating_sub(at) >= CLIMB_AFTER {
                        progress.want_connections.store(want + 1, Ordering::Relaxed);
                        progress.lowered_at.store(if want + 1 >= began { 0 } else { now_secs() }, Ordering::Relaxed);
                    }
                }
                // more connections wanted (Options): as many more as there are
                // parts worth sharing
                if first_error.is_none() && segmented && !child.is_cancelled() {
                    let (running, leaving) = {
                        let c = crew.lock().unwrap();
                        (c.running, c.leaving)
                    };
                    let missing = wanted().saturating_sub(running.saturating_sub(leaving));
                    for _ in 0..missing {
                        if !has_work(&segs, job.min_split) {
                            break;
                        }
                        spawn(&mut set);
                    }
                }
            }
        }
    }
    if file.sync_all().is_ok() {
        for s in segs.lock().unwrap().iter_mut() {
            s.on_disk = None;
        }
    }

    if cancel.is_cancelled() {
        return Err(Cancelled.into());
    }
    if let Some(e) = first_error {
        return Err(e);
    }
    let table = segs.lock().unwrap();
    if segmented && !segments::all_finished(&table) {
        bail!("download ended before all parts were received");
    }
    Ok(())
}

// ---------------------------------------------------------------- servers that allow fewer connections

/// Sites that answered "too many requests" (HTTP 429): how many connections
/// they took, and when that was seen (seconds since 1970). Their next
/// downloads start with that many, for a day: a site that was busy once
/// (or a big shared server, a CDN) is not held at one or two connections
/// for ever.
/// site -> (connections, when); and whether it changed since it was kept
type Learned = (std::collections::BTreeMap<String, (u32, u64)>, bool);
static LEARNED: std::sync::LazyLock<Mutex<Learned>> =
    std::sync::LazyLock::new(|| Mutex::new((Default::default(), false)));

/// How long a learned limit holds.
const LEARNED_FOR: u64 = 24 * 3600;

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The site of an address: download2430.mediafire.com -> mediafire.com
/// (its many download servers count as one).
pub fn site_of(url: &str) -> Option<String> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return Some(host);
    }
    let parts: Vec<&str> = host.split('.').filter(|p| !p.is_empty()).collect();
    let keep = match parts.as_slice() {
        // example.co.uk, example.com.eg
        [.., second, last] if last.len() == 2 && matches!(*second, "co" | "com" | "net" | "org" | "gov" | "edu" | "ac") => 3,
        _ => 2,
    };
    Some(parts[parts.len().saturating_sub(keep)..].join("."))
}

/// How many connections the site of `url` took (None: no limit seen lately).
pub fn learned_limit(url: &str) -> Option<u32> {
    let site = site_of(url)?;
    let l = LEARNED.lock().unwrap();
    l.0.get(&site).filter(|(_, at)| now_secs().saturating_sub(*at) < LEARNED_FOR).map(|(n, _)| *n)
}

fn learn(url: &str, n: u32) {
    let Some(site) = site_of(url) else { return };
    let mut l = LEARNED.lock().unwrap();
    let n = n.max(1);
    let now = now_secs();
    let lower = match l.0.get(&site) {
        None => true,
        Some((old, at)) => n < *old || now.saturating_sub(*at) >= LEARNED_FOR,
    };
    if lower {
        l.0.insert(site, (n, now));
        l.1 = true;
    }
}

/// The limits seen before (at start), as kept by `take_learned_changes`:
/// {"site": [n, seconds]}. Older files ({"site": n}, no time) are not taken:
/// their limits were kept for ever, and are let go now.
pub fn set_learned(map: std::collections::BTreeMap<String, serde_json::Value>) {
    let now = now_secs();
    let mut l = LEARNED.lock().unwrap();
    l.0 = map
        .into_iter()
        .filter_map(|(site, v)| {
            let n = v.get(0)?.as_u64()? as u32;
            let at = v.get(1)?.as_u64()?;
            (n >= 1 && now.saturating_sub(at) < LEARNED_FOR).then_some((site, (n, at)))
        })
        .collect();
    l.1 = false;
}

/// The limits, when new ones were seen since the last call (to be kept).
pub fn take_learned_changes() -> Option<std::collections::BTreeMap<String, (u32, u64)>> {
    let mut l = LEARNED.lock().unwrap();
    if !l.1 {
        return None;
    }
    l.1 = false;
    let now = now_secs();
    Some(l.0.iter().filter(|(_, (_, at))| now.saturating_sub(*at) < LEARNED_FOR).map(|(k, v)| (k.clone(), *v)).collect())
}

/// Fewer connections from now on (never more than wanted already).
fn lower_wanted(progress: &Progress, job_conns: u32, n: u32) {
    let n = n.max(1);
    progress.lowered_at.store(now_secs(), Ordering::Relaxed);
    let _ = progress.want_connections.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
        let now = if cur == 0 { job_conns } else { cur };
        (n < now).then_some(n)
    });
}

/// "Retry-After: 30" -> 30 s (a date or nothing: None).
fn retry_after_of(resp: &reqwest::Response) -> Option<u64> {
    resp.headers().get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim().parse::<u64>().ok().filter(|s| *s <= 600)
}

/// A part could be taken by one more connection: one nobody works on, or
/// one big enough to be shared.
fn has_work(segs: &SegTable, min_split: u64) -> bool {
    let t = segs.lock().unwrap();
    let min = min_split.clamp(1, TAIL_SPLIT) * 2;
    t.iter().any(|s| !s.is_finished() && (!s.active || s.remaining() >= min))
}

struct Worker {
    shared: Arc<Shared>,
    job: HttpJob,
    segs: SegTable,
    progress: Arc<Progress>,
    file: Arc<File>,
    cancel: CancellationToken,
    segmented: bool,
    crew: Arc<Mutex<Crew>>,
    wanted: Arc<dyn Fn() -> u32 + Send + Sync>,
    /// the server's "Retry-After" of its last "too many requests" (seconds)
    retry_after: std::sync::atomic::AtomicU64,
}

/// Counts the connection out when it ends, however it ends.
struct OutOfCrew(Arc<Mutex<Crew>>, bool);
impl Drop for OutOfCrew {
    fn drop(&mut self) {
        let mut c = self.0.lock().unwrap();
        c.running = c.running.saturating_sub(1);
        if self.1 {
            c.leaving = c.leaving.saturating_sub(1);
        }
    }
}

impl Worker {
    /// A connection was dropped: when that happens again and again (6 in a
    /// minute) while the download as a whole goes on, the server allows
    /// fewer connections: half as many are wanted. True when that was decided.
    fn server_drops_too_many(&self) -> bool {
        let now = std::time::Instant::now();
        let bytes = self.progress.bytes.load(Ordering::Relaxed);
        let mut c = self.crew.lock().unwrap();
        c.drops.push_back((now, bytes));
        while c.drops.front().is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(60)) {
            c.drops.pop_front();
        }
        let running = c.running.saturating_sub(c.leaving);
        let first_bytes = c.drops.front().map(|(_, b)| *b).unwrap_or(bytes);
        // the others still bring data (not the internet that is gone)
        let going_on = bytes.saturating_sub(first_bytes) >= 1 << 20;
        if c.drops.len() >= 6 && going_on && running > 2 {
            c.drops.clear();
            drop(c);
            lower_wanted(&self.progress, self.job.connections.max(1), (running / 2).max(2));
            true
        } else {
            false
        }
    }

    /// More connections run than wanted now: this one stops (counted at once,
    /// so only as many stop as needed).
    /// The other connections still bring data (watched for 2 seconds): the
    /// link works, this connection alone was refused.
    async fn others_bring_data(&self) -> bool {
        let others = {
            let c = self.crew.lock().unwrap();
            c.running.saturating_sub(c.leaving)
        };
        if others < 2 {
            return false;
        }
        let before = self.progress.bytes.load(Ordering::Relaxed);
        tokio::select! {
            _ = tokio::time::sleep(OTHERS_WATCH) => {}
            _ = self.cancel.cancelled() => return false,
        }
        self.progress.bytes.load(Ordering::Relaxed) > before
    }

    fn should_leave(&self) -> bool {
        let want = (self.wanted)();
        let mut c = self.crew.lock().unwrap();
        if c.running.saturating_sub(c.leaving) > want {
            c.leaving += 1;
            true
        } else {
            false
        }
    }

    async fn run(self) -> anyhow::Result<()> {
        let mut out = OutOfCrew(self.crew.clone(), false);
        let r = self.work().await;
        match r {
            Err(e) if e.is::<Leave>() => {
                out.1 = true;
                Ok(())
            }
            other => other,
        }
    }

    async fn work(&self) -> anyhow::Result<()> {
        loop {
            if self.segmented && self.should_leave() {
                return Err(Leave.into());
            }
            let idx = {
                let mut table = self.segs.lock().unwrap();
                if self.segmented {
                    // the last parts too are shared, in smaller pieces
                    segments::claim_work(&mut table, self.job.min_split)
                        .or_else(|| segments::split_largest(&mut table, self.job.min_split.min(TAIL_SPLIT)))
                } else {
                    let s = &mut table[0];
                    if s.active || s.is_finished() {
                        None
                    } else {
                        s.active = true;
                        Some(0)
                    }
                }
            };
            let Some(idx) = idx else { return Ok(()) };
            let result = self.download_segment(idx).await;
            {
                let mut table = self.segs.lock().unwrap();
                if let Some(s) = table.get_mut(idx) {
                    s.active = false;
                }
            }
            result?;
        }
    }

    /// Download one segment, retrying with back-off and continuing from where
    /// the previous attempt stopped.
    async fn download_segment(&self, idx: usize) -> anyhow::Result<()> {
        let mut attempt = 0u32;
        let mut net_tries = 0u32;
        let mut too_many = 0u32;
        loop {
            if self.cancel.is_cancelled() {
                return Err(Cancelled.into());
            }
            let before = self.segs.lock().unwrap()[idx].pos;
            let outcome = tokio::select! {
                r = self.fetch_once(idx) => r,
                _ = self.cancel.cancelled() => return Err(Cancelled.into()),
            };
            match outcome {
                Ok(()) => {
                    if self.segs.lock().unwrap()[idx].is_finished() {
                        return Ok(());
                    }
                    attempt = 0; // one request done, ask for the next piece
                }
                Err(e) if e.is::<Leave>() => return Err(e),
                Err(e) => {
                    let after = self.segs.lock().unwrap()[idx].pos;
                    if after > before {
                        attempt = 0; // it was making progress, don't give up yet
                        net_tries = 0;
                    }
                    let network = is_network_error(&e);
                    let status = http_status_of(&e);
                    if status == Some(429) {
                        // "too many requests": the site allows fewer connections
                        // at a time. One less from now on (this one goes, its
                        // part stays for the others), and the site's next
                        // downloads start with that many; the last ones wait
                        // as long as the site asks and go on.
                        if self.segmented {
                            let now = {
                                let c = self.crew.lock().unwrap();
                                c.running.saturating_sub(c.leaving)
                            };
                            let fewer = now.saturating_sub(1).max(1);
                            lower_wanted(&self.progress, self.job.connections.max(1), fewer);
                            learn(&self.job.url, fewer);
                            if self.should_leave() {
                                return Err(Leave.into());
                            }
                        }
                        too_many += 1;
                        if too_many > 40 {
                            return Err(e);
                        }
                        let asked = self.retry_after.swap(0, Ordering::Relaxed);
                        let wait = if asked > 0 { asked } else { (3 * too_many as u64).min(60) };
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(wait)) => {},
                            _ = self.cancel.cancelled() => return Err(Cancelled.into()),
                        }
                        continue;
                    }
                    if matches!(status, Some(401 | 403)) && self.segmented && !crate::http::wants_captcha(&e) && self.others_bring_data().await {
                        // refused while the other connections of the same
                        // link go on: not an expired link but one connection
                        // too many (pixeldrain and other sites answer 403, not
                        // 429). One less from now on, as for 429; this one
                        // goes and its part stays for the others
                        let now = {
                            let c = self.crew.lock().unwrap();
                            c.running.saturating_sub(c.leaving)
                        };
                        let fewer = now.saturating_sub(1).max(1);
                        lower_wanted(&self.progress, self.job.connections.max(1), fewer);
                        learn(&self.job.url, fewer);
                        if self.should_leave() {
                            return Err(Leave.into());
                        }
                        too_many += 1;
                        if too_many > 40 {
                            return Err(e);
                        }
                        continue;
                    }
                    if network && self.segmented && !format!("{e:#}").contains("slow connection") && self.server_drops_too_many() {
                        // the server keeps dropping connections while the
                        // others go on (sites that allow only a few): half as
                        // many from now on; this one goes, its part stays
                        if self.should_leave() {
                            return Err(Leave.into());
                        }
                    }
                    if network {
                        // no internet / reset / stall: wait as long as it takes,
                        // the part keeps what it already has
                        net_tries += 1;
                        self.progress.waiting.store(true, Ordering::Relaxed);
                    } else {
                        attempt += 1;
                        // an expired or removed link won't come back by itself
                        let gone = matches!(status, Some(401 | 403 | 404 | 410));
                        let limit = if gone { self.job.retries.min(2) } else { self.job.retries };
                        if attempt > limit {
                            if gone {
                                return Err(e.context(
                                    "the link has expired or the file was removed - use \"Refresh download address\"",
                                ));
                            }
                            return Err(e);
                        }
                    }
                    if !self.segmented {
                        // can't resume without ranges: restart from zero
                        let mut table = self.segs.lock().unwrap();
                        let s = &mut table[idx];
                        self.progress.bytes.fetch_sub(s.done(), Ordering::Relaxed);
                        s.pos = s.start;
                        let _ = self.file.set_len(0);
                    }
                    let made_again = format!("{e:#}").contains("slow connection");
                    let backoff = if made_again {
                        // a slow connection made again: at once
                        Duration::ZERO
                    } else if network {
                        reconnect_delay(net_tries)
                    } else {
                        Duration::from_secs((1u64 << attempt.min(5)).min(30))
                    };
                    tokio::select! {
                        _ = tokio::time::sleep(backoff) => {},
                        // another part got through: the internet is back
                        _ = self.progress.back_online.notified() => {},
                        _ = self.cancel.cancelled() => return Err(Cancelled.into()),
                    }
                }
            }
        }
    }

    async fn fetch_once(&self, idx: usize) -> anyhow::Result<()> {
        let (pos, end) = {
            let table = self.segs.lock().unwrap();
            (table[idx].pos, table[idx].end)
        };
        if pos >= end {
            return Ok(());
        }
        // where this single request stops (the segment may need several)
        let req_end = match self.job.max_request {
            Some(limit) if self.segmented => end.min(pos + limit.max(1)),
            _ => end,
        };
        let mut rb = request(&crate::http::client_for(&self.shared.client(), &self.job.url), &self.job.url, &self.job.headers, &self.job.user_agent);
        if self.segmented {
            rb = rb.header(RANGE, format!("bytes={}-{}", pos, req_end - 1));
        }
        // the answer (its headers) must come: a server that keeps quiet does
        // not hold this connection for ever
        let mut resp = match tokio::time::timeout(HEADERS_TIMEOUT, rb.send_ordered()).await {
            Ok(r) => r?,
            Err(_) => return Err(NetDrop("the server did not answer (timed out)").into()),
        };
        let status = resp.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            self.retry_after.store(retry_after_of(&resp).unwrap_or(0), Ordering::Relaxed);
        }
        if status == StatusCode::FORBIDDEN {
            // a site that wants a "not a robot" check (pixeldrain and others
            // say so in the answer): no new link or try helps, the browser must
            let body = tokio::time::timeout(Duration::from_secs(3), resp.chunk()).await.ok().and_then(|c| c.ok()).flatten();
            if body.is_some_and(|b| String::from_utf8_lossy(&b[..b.len().min(4096)]).to_ascii_lowercase().contains("captcha")) {
                return Err(anyhow!(HttpStatus(403)).context(crate::http::CAPTCHA));
            }
            return Err(HttpStatus(403).into());
        }
        if self.segmented {
            if status != StatusCode::PARTIAL_CONTENT {
                if status.is_client_error() || status.is_server_error() {
                    return Err(HttpStatus(status.as_u16()).into());
                }
                bail!("server did not accept the byte range (HTTP {})", status.as_u16());
            }
            // the part must be the one asked for (and of the same file):
            // anything else is never written
            if let Some((start, total)) = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range)
            {
                if start != pos {
                    return Err(NetDrop("the server sent another part of the file").into());
                }
                if let (Some(total), Some(known)) = (total, self.job.total_size) {
                    if total != known {
                        bail!("the link now gives another file: not the same file ({total} bytes instead of {known})");
                    }
                }
            }
        } else if !status.is_success() {
            return Err(HttpStatus(status.as_u16()).into());
        }
        let _guard = ConnGuard::new(&self.progress);
        // how fast this connection is compared with the whole download
        let mut window = std::time::Instant::now();
        let (mut mine, mut all_before) = (0u64, self.progress.bytes.load(Ordering::Relaxed));
        let mut slow_windows = 0u32;

        loop {
            let chunk = match tokio::time::timeout(STALL_TIMEOUT, resp.chunk()).await {
                Ok(r) => r?,
                Err(_) => return Err(NetDrop("connection timed out").into()),
            };
            let Some(chunk) = chunk else { break };
            if self.segmented && self.should_leave() {
                // fewer connections wanted: this one goes, its part stays for the others
                return Err(Leave.into());
            }
            mine += chunk.len() as u64;
            let spent = window.elapsed();
            if self.segmented && spent >= SLOW_WINDOW {
                let all_now = self.progress.bytes.load(Ordering::Relaxed);
                let conns = self.progress.connections.load(Ordering::Relaxed).max(1) as u64;
                let secs = spent.as_secs().max(1);
                let my_rate = mine / secs;
                let average = all_now.saturating_sub(all_before) / secs / conns;
                // much slower than the others (and the limiter is not the reason)
                let limited = self.shared.limiter.rate() > 0 || self.job.limiter.rate() > 0;
                // alone (the others are done): held to the best the
                // connections brought before (a slow last connection kept
                // the download at 90-99 % for ever)
                let best = self.crew.lock().unwrap().best;
                // (only near the end: a slow line is not made again and again)
                let near_end = self.job.total_size.is_some_and(|t| segments::downloaded(&self.segs.lock().unwrap()) >= t / 10 * 8);
                let compare = if conns >= 2 { average } else if near_end && self.job.connections >= 2 { best } else { u64::MAX };
                if !limited && my_rate < SLOW_BYTES_PER_SEC && my_rate.saturating_mul(SLOW_SHARE) < compare && compare != u64::MAX {
                    slow_windows += 1;
                } else {
                    slow_windows = 0;
                }
                if slow_windows >= 2 {
                    return Err(NetDrop("slow connection, made again").into());
                }
                window = std::time::Instant::now();
                mine = 0;
                all_before = all_now;
            }
            self.shared.limiter.acquire(chunk.len()).await;
            self.job.limiter.acquire(chunk.len()).await;
            let finished = {
                let mut table = self.segs.lock().unwrap();
                let s = &mut table[idx];
                // `end` may have moved down because another worker split us
                let stop = s.end.min(req_end);
                let room = stop.saturating_sub(s.pos);
                let take = (chunk.len() as u64).min(room) as usize;
                if take > 0 {
                    write_at(&self.file, &chunk[..take], s.pos)?;
                    s.pos += take as u64;
                    self.progress.bytes.fetch_add(take as u64, Ordering::Relaxed);
                    self.progress.online();
                }
                s.pos >= stop
            };
            if finished {
                return Ok(()); // dropping `resp` closes the connection
            }
        }

        // the server closed the stream
        let mut table = self.segs.lock().unwrap();
        let s = &mut table[idx];
        if !self.segmented && (self.job.total_size.is_none() || s.end == u64::MAX) {
            s.end = s.pos; // unknown size: end of stream means done
            return Ok(());
        }
        if s.pos >= s.end.min(req_end) {
            Ok(())
        } else {
            Err(NetDrop("connection closed early").into())
        }
    }
}

/// "bytes 100-199/1000" -> (100, Some(1000)); "bytes 100-199/*" -> (100, None)
fn parse_content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let rest = v.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    let total = total.trim().parse().ok();
    Some((start, total))
}

#[cfg(test)]
mod learned_tests {
    use super::*;

    #[test]
    fn sites_and_their_limits() {
        assert_eq!(site_of("https://download2430.mediafire.com/x/y.rar").as_deref(), Some("mediafire.com"));
        assert_eq!(site_of("https://a.b.example.co.uk/f").as_deref(), Some("example.co.uk"));
        assert_eq!(site_of("http://127.0.0.1:8080/f").as_deref(), Some("127.0.0.1"));
        learn("https://download1.mediafire.test/a", 8);
        learn("https://download2.mediafire.test/b", 12);
        assert_eq!(learned_limit("https://download9.mediafire.test/c"), Some(8));
        learn("https://download2.mediafire.test/b", 5);
        assert_eq!(learned_limit("https://x.mediafire.test/"), Some(5));
        // a limit from a file of an older version (no time): let go
        let old: std::collections::BTreeMap<String, serde_json::Value> =
            serde_json::from_str(r#"{"old.test": 1, "new.test": [3, 9999999999], "gone.test": [2, 1]}"#).unwrap();
        set_learned(old);
        assert_eq!(learned_limit("https://old.test/"), None);
        assert_eq!(learned_limit("https://gone.test/"), None, "older than a day");
        let p = Progress::default();
        lower_wanted(&p, 32, 31);
        lower_wanted(&p, 32, 40);
        assert_eq!(p.want_connections.load(Ordering::Relaxed), 31);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_content_range;

    #[test]
    fn content_range() {
        assert_eq!(parse_content_range("bytes 100-199/1000"), Some((100, Some(1000))));
        assert_eq!(parse_content_range("bytes 0-0/*"), Some((0, None)));
        assert_eq!(parse_content_range(" bytes  5-9/10 "), Some((5, Some(10))));
        assert_eq!(parse_content_range("bytes */1000"), None);
        assert_eq!(parse_content_range("items 1-2/3"), None);
    }
}
