//! Multi-connection download of a normal HTTP file.
//!
//! Every connection ("worker") asks the segment table for work, downloads its
//! byte range with an HTTP Range request and writes straight into the right
//! offset of the `.sgpart` file. When a worker finishes it steals half of the
//! biggest remaining segment, so all connections stay busy until the end.

use crate::fileio::{make_sparse, write_at};
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
}

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
    if let Some(dir) = job.part_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let segmented = job.resumable && job.total_size.is_some();
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&job.part_path)?;

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
                n => n.clamp(1, 32),
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
        };
        set.spawn(ctx.run());
    };
    for _ in 0..wanted() {
        spawn(&mut set);
    }

    let mut first_error: Option<anyhow::Error> = None;
    let mut look = tokio::time::interval(Duration::from_millis(500));
    look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
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
            _ = look.tick() => {
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
    file.sync_all().ok();

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

/// A part could be taken by one more connection: one nobody works on, or
/// one big enough to be shared.
fn has_work(segs: &SegTable, min_split: u64) -> bool {
    let t = segs.lock().unwrap();
    let min = min_split.max(1) * 2;
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
    /// More connections run than wanted now: this one stops (counted at once,
    /// so only as many stop as needed).
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
                    segments::claim_work(&mut table, self.job.min_split)
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
                    let backoff = if network {
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
        let mut rb = request(&self.shared.client(), &self.job.url, &self.job.headers, &self.job.user_agent);
        if self.segmented {
            rb = rb.header(RANGE, format!("bytes={}-{}", pos, req_end - 1));
        }
        // the answer (its headers) must come: a server that keeps quiet does
        // not hold this connection for ever
        let mut resp = match tokio::time::timeout(HEADERS_TIMEOUT, rb.send()).await {
            Ok(r) => r?,
            Err(_) => return Err(NetDrop("the server did not answer (timed out)").into()),
        };
        let status = resp.status();
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
                if !limited && conns >= 2 && my_rate < SLOW_BYTES_PER_SEC && my_rate * SLOW_SHARE < average {
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
