//! The download manager: owns every download, runs the queue, the scheduler,
//! speed measurement and persistence. The UI only talks to this type.

use crate::config::{parse_hhmm, AfterDone, QueueDef, Settings, MAIN_QUEUE};
use crate::filename;
use crate::hls::{self, HlsJob};
use crate::http::{self, build_client_with, Progress, ProxyConfig, Shared};
use crate::http_download::{self, Cancelled, HttpJob, SegTable};
use crate::limiter::SpeedLimiter;
use crate::model::*;
use crate::mux;
use crate::ytdlp;
use crate::torrent;
use crate::segments;
use crate::store;
use crate::system;
use anyhow::anyhow;
use chrono::{Datelike, Timelike, Utc};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

struct Entry {
    rec: DownloadRecord,
    segs: SegTable,
    /// Dash downloads only: segment table of the audio stream
    audio_segs: SegTable,
    /// this download's own speed limit (on top of the global one)
    limiter: Arc<SpeedLimiter>,
    progress: Arc<Progress>,
    /// `Some` while a task is working on this download
    task: Option<CancellationToken>,
    speed: SpeedMeter,
    /// bytes counted for the download quota so far
    quota_prev: u64,
}

/// Things that happened, for sounds and messages in the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Completed(DownloadId),
    Failed(DownloadId),
    QueueStarted(u64),
    QueueDone(u64),
    /// Options > Connection > download limit reached; downloads were stopped
    QuotaReached,
    /// the queue wants this finished file opened
    OpenFile(DownloadId),
}

#[derive(Default)]
struct SpeedMeter {
    /// (time, bytes) samples of the last ~2 seconds
    samples: std::collections::VecDeque<(Instant, u64)>,
    value: f64,
}

struct Inner {
    entries: BTreeMap<DownloadId, Entry>,
    next_id: DownloadId,
    dirty: bool,
    last_save: Instant,
    sched_minute: Option<(u32, u32, u32)>,
    completed: Vec<DownloadId>,
    exit_requested: bool,
    when_all_done: AfterDone,
    all_done_seen_active: bool,
    events: Vec<Event>,
    /// shut down / sleep / hibernate waiting for the countdown window
    power_request: Option<AfterDone>,
    /// (time, bytes) of the download quota window
    quota_log: VecDeque<(Instant, u64)>,
    /// stopped because of the quota; started again when it allows
    quota_paused: Vec<DownloadId>,
    quota_blocked: bool,
    /// queues started (by hand or by their schedule) whose "when done" is armed
    queue_armed: HashSet<u64>,
}

pub struct Manager {
    inner: Mutex<Inner>,
    settings: RwLock<Settings>,
    shared: Arc<Shared>,
    proxy: Mutex<ProxyConfig>,
    rt: Handle,
    data_dir: PathBuf,
    /// the hidden torrent program (started when the first torrent runs)
    torrents: Arc<torrent::Aria2>,
    /// downloads whose link stopped working: the next start gets a new link
    /// and goes on only when it is the same file (size, ranges)
    relink: Mutex<HashSet<DownloadId>>,
}

/// Default place for settings.json and downloads.json
/// (%APPDATA%\Ezoo\EzooDownload\data on Windows). The list and settings of
/// the earlier "SwiftGet" name are carried over the first time.
pub fn default_data_dir() -> PathBuf {
    let dir = directories::ProjectDirs::from("com", "Ezoo", "EzooDownload")
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("ezoo-data"));
    if !dir.exists() {
        if let Some(old) = directories::ProjectDirs::from("com", "SwiftGet", "SwiftGet").map(|p| p.data_dir().to_path_buf()) {
            if old.exists() {
                let _ = std::fs::create_dir_all(&dir);
                for f in ["settings.json", "downloads.json"] {
                    let _ = std::fs::copy(old.join(f), dir.join(f));
                }
            }
        }
    }
    dir
}

/// The new link of a download whose old link stopped working gives the same
/// file: as big as before, a web page it is not, and it takes part requests.
fn same_file(p: &ProbeResult, size: Option<u64>) -> anyhow::Result<()> {
    if p.is_web_page() || !p.resumable || size.is_none() || p.total_size != size {
        anyhow::bail!(
            "the new link is not the same file ({} instead of {}) - nothing was written; use \"Refresh download address\"",
            p.total_size.map(|n| n.to_string()).unwrap_or_else(|| "unknown size".into()),
            size.map(|n| n.to_string()).unwrap_or_else(|| "unknown".into())
        );
    }
    Ok(())
}

impl Manager {
    pub fn new(rt: Handle, data_dir: PathBuf) -> anyhow::Result<Arc<Manager>> {
        std::fs::create_dir_all(&data_dir)?;
        let settings = Settings::load(&data_dir.join("settings.json"));
        ytdlp::set_mp3_kbps(settings.mp3_kbps);
        settings.create_folders();
        let proxy = ProxyConfig::from_settings(&settings);
        // a broken manual proxy must not keep the app from starting
        let client = build_client_with(&proxy).or_else(|_| build_client_with(&ProxyConfig { mode: 1, ..Default::default() }))?;
        let shared = Arc::new(Shared::new(client, SpeedLimiter::new(settings.effective_limit())));

        let mut entries = BTreeMap::new();
        let mut next_id = 1;
        for mut rec in store::load_downloads(&data_dir.join("downloads.json")) {
            if !matches!(rec.status, Status::Completed | Status::Failed | Status::Paused) {
                rec.status = Status::Paused;
            }
            let progress = Arc::new(Progress::default());
            let bytes = if rec.kind != DownloadKind::Hls && !rec.segments.is_empty() {
                segments::downloaded(&rec.segments) + segments::downloaded(&rec.audio_segments)
            } else {
                rec.downloaded
            };
            progress.bytes.store(bytes, Ordering::Relaxed);
            progress.parts_total.store(rec.parts_total, Ordering::Relaxed);
            progress.parts_done.store(rec.parts_done, Ordering::Relaxed);
            next_id = next_id.max(rec.id + 1);
            entries.insert(
                rec.id,
                Entry {
                    segs: Arc::new(Mutex::new(rec.segments.clone())),
                    audio_segs: Arc::new(Mutex::new(rec.audio_segments.clone())),
                    limiter: Arc::new(SpeedLimiter::new(rec.speed_limit_kb.unwrap_or(0) * 1024)),
                    rec,
                    progress,
                    task: None,
                    speed: SpeedMeter::default(),
                    quota_prev: 0,
                },
            );
        }
        let startup_queues: Vec<u64> =
            settings.queues.iter().filter(|q| q.schedule.on_startup).map(|q| q.id).collect();

        let mgr = Arc::new(Manager {
            inner: Mutex::new(Inner {
                entries,
                next_id,
                dirty: false,
                last_save: Instant::now(),
                sched_minute: None,
                completed: Vec::new(),
                exit_requested: false,
                when_all_done: AfterDone::Nothing,
                all_done_seen_active: false,
                events: Vec::new(),
                power_request: None,
                quota_log: VecDeque::new(),
                quota_paused: Vec::new(),
                quota_blocked: false,
                queue_armed: HashSet::new(),
            }),
            settings: RwLock::new(settings),
            shared,
            proxy: Mutex::new(proxy),
            rt: rt.clone(),
            torrents: Arc::new(torrent::Aria2::new(&data_dir.join("torrents"))),
            data_dir,
            relink: Mutex::new(HashSet::new()),
        });

        for q in startup_queues {
            mgr.start_queue(q);
        }
        let m = mgr.clone();
        rt.spawn(async move {
            let mut every = tokio::time::interval(Duration::from_millis(200));
            loop {
                every.tick().await;
                m.tick();
            }
        });
        Ok(mgr)
    }

    // ---------------------------------------------------------------- settings

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn set_settings(self: &Arc<Self>, s: Settings) -> anyhow::Result<()> {
        let before = self.settings.read().unwrap().connections();
        if s.connections() != before {
            // Options > connections: every download that is not finished takes
            // the new number, a running one at once (no restart)
            let mut inner = self.inner.lock().unwrap();
            for e in inner.entries.values_mut() {
                if e.rec.status == Status::Completed || e.rec.kind == DownloadKind::Torrent {
                    continue;
                }
                let n = s.connections_for(&e.rec.url, s.connections());
                e.rec.connections = n;
                e.progress.want_connections.store(n, Ordering::Relaxed);
            }
            inner.dirty = true;
        }
        self.shared.limiter.set_rate(s.effective_limit());
        ytdlp::set_mp3_kbps(s.mp3_kbps);
        let proxy = ProxyConfig::from_settings(&s);
        if *self.proxy.lock().unwrap() != proxy {
            self.shared.set_client(build_client_with(&proxy)?);
            *self.proxy.lock().unwrap() = proxy;
        }
        s.save(&self.data_dir.join("settings.json"))?;
        s.create_folders();
        *self.settings.write().unwrap() = s;
        self.pump();
        Ok(())
    }

    pub fn runtime(&self) -> &Handle {
        &self.rt
    }

    // ---------------------------------------------------------------- actions

    /// Look at a URL without downloading it (for the "add" dialog).
    pub async fn probe(&self, url: &str, headers: &RequestHeaders) -> anyhow::Result<ProbeResult> {
        let (ua, headers) = {
            let s = self.settings.read().unwrap();
            (s.user_agent.clone(), with_login(&s, url, headers))
        };
        http::probe(&self.shared.client(), url, &headers, &ua).await
    }

    /// The HTTP client (with the proxy settings), for the site grabber.
    /// A .torrent file from a web address (a link clicked in the browser):
    /// fetched at once and kept in our data folder, so the torrent window can
    /// show what it shares. Never more than a few MB, and only a real
    /// torrent file is kept.
    pub async fn fetch_torrent_file(&self, url: &str, headers: &RequestHeaders) -> anyhow::Result<PathBuf> {
        const MAX: usize = 16 << 20;
        let ua = self.settings().user_agent.clone();
        let mut resp = http::request(&self.client(), url, headers, &ua).send().await?.error_for_status()?;
        if resp.content_length().is_some_and(|n| n > MAX as u64) {
            anyhow::bail!("too big for a .torrent file");
        }
        let mut data = Vec::new();
        while let Some(chunk) = resp.chunk().await? {
            data.extend_from_slice(&chunk);
            if data.len() > MAX {
                anyhow::bail!("too big for a .torrent file");
            }
        }
        let info = torrent::parse_torrent(&data)?;
        let dir = self.data_dir.join("torrents").join("from-browser");
        std::fs::create_dir_all(&dir)?;
        // a name of our own: never a path from the server
        let stem: String = info.name.chars().filter(|c| c.is_alphanumeric() || " -_.()[]".contains(*c)).take(80).collect();
        let stem = if stem.trim().is_empty() { "download".to_string() } else { stem.trim().to_string() };
        let path = dir.join(filename::unique_name(&dir, &format!("{stem}.torrent")));
        std::fs::write(&path, &data)?;
        Ok(path)
    }

    pub fn client(&self) -> reqwest::Client {
        self.shared.client()
    }

    pub fn add(self: &Arc<Self>, req: NewDownload) -> DownloadId {
        let settings = self.settings();
        let probe = req.probe.clone();
        let auto_name = req.file_name.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true) && req.probe.is_none();
        let wanted = req
            .file_name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(filename::sanitize)
            .or_else(|| probe.as_ref().map(|p| p.file_name.clone()))
            .or_else(|| filename::from_url(&req.url).map(|n| filename::sanitize(&n)))
            .unwrap_or_else(|| "download".into());
        // torrents: the name they share under, the category of their biggest file
        let is_torrent = torrent::is_torrent_source(&req.url);
        let torrent_info = (is_torrent && !torrent::is_magnet(&req.url))
            .then(|| std::fs::read(&req.url).ok().and_then(|d| torrent::parse_torrent(&d).ok()))
            .flatten();
        let wanted = if is_torrent && req.file_name.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true) {
            torrent_info
                .as_ref()
                .map(|i| i.name.clone())
                .or_else(|| torrent::magnet_name(&req.url))
                .map(|n| filename::sanitize(&n))
                .unwrap_or_else(|| "torrent".into())
        } else {
            wanted
        };
        let category = match torrent_info.as_ref().and_then(|i| i.files.iter().max_by_key(|f| f.size)) {
            Some(big) => settings.category_of(&big.path, ""),
            None => settings.category_of(&wanted, &req.url),
        };
        let queue_folder = req
            .queue
            .and_then(|q| settings.queue(q))
            .and_then(|q| q.folder.clone())
            .filter(|d| !d.as_os_str().is_empty());
        let dir = req
            .save_dir
            .clone()
            .filter(|d| !d.as_os_str().is_empty())
            .or(queue_folder)
            .unwrap_or_else(|| settings.dir_for(&category));
        let _ = std::fs::create_dir_all(&dir);

        let audio_url = req.audio_url.clone().filter(|u| !u.trim().is_empty());
        let kind = if is_torrent {
            DownloadKind::Torrent
        } else if audio_url.is_some() {
            DownloadKind::Dash
        } else {
            probe.as_ref().map(|p| p.kind).unwrap_or_else(|| {
            if req.url.to_ascii_lowercase().split('?').next().unwrap_or("").ends_with(".m3u8") {
                DownloadKind::Hls
            } else {
                DownloadKind::Http
            }
        })
        };

        let mut inner = self.inner.lock().unwrap();
        // a torrent keeps its own name: that is the folder / file it makes
        let file_name = if is_torrent { wanted.clone() } else { free_name(&inner, &dir, &wanted) };
        let id = inner.next_id;
        inner.next_id += 1;
        // our own copy of the .torrent file (the one given may be deleted)
        let mut url = req.url.clone();
        if is_torrent && !torrent::is_magnet(&url) {
            let keep = self.data_dir.join("torrents").join(format!("{id}.torrent"));
            if std::fs::create_dir_all(keep.parent().unwrap_or(&self.data_dir)).is_ok() && std::fs::copy(&url, &keep).is_ok() {
                url = keep.to_string_lossy().into_owned();
            }
        }
        let early = req.early;
        let rec = DownloadRecord {
            id,
            url,
            final_url: probe.as_ref().map(|p| p.final_url.clone()),
            file_name,
            save_dir: dir,
            kind,
            // for Dash the real total (video + audio) is known once it starts
            total_size: match kind {
                DownloadKind::Dash => None,
                DownloadKind::Torrent => torrent_info.as_ref().map(|i| i.total),
                _ => probe.as_ref().and_then(|p| p.total_size),
            },
            resumable: probe.as_ref().map(|p| p.resumable).unwrap_or(false),
            segments: Vec::new(),
            audio_url,
            audio_segments: Vec::new(),
            video_size: None,
            source_page: req.source_page.clone(),
            source_key: req.source_key.clone(),
            queue: req.queue,
            auto_name,
            description: req.description.clone().filter(|d| !d.trim().is_empty()),
            speed_limit_kb: None,
            on_complete: AfterDone::Nothing,
            downloaded: 0,
            parts_total: 0,
            parts_done: 0,
            status: if req.start { Status::Queued } else { Status::Paused },
            error: None,
            headers: req.headers,
            connections: req.connections.unwrap_or(settings.connections()).clamp(1, 32),
            category,
            added_at: Utc::now(),
            finished_at: None,
            temp_dir: settings
                .temp_dir
                .clone()
                .filter(|t| !t.as_os_str().is_empty() && std::fs::create_dir_all(t).is_ok()),
            queue_pos: id,
            last_try: None,
            server_time: probe.as_ref().and_then(|p| p.last_modified),
            retry_count: 0,
            pick_quality: req.pick_quality.clone(),
            work_stem: None,
            torrent_select: req.torrent_select.clone().filter(|s| !s.trim().is_empty()),
            torrent_files: Vec::new(),
        };
        let mut rec = rec;
        if early {
            // parts get a fixed name in a fixed folder: name and folder can still change
            rec.work_stem = Some(format!(".ezoo-{id}"));
            if rec.temp_dir.is_none() {
                rec.temp_dir = Some(rec.save_dir.clone());
            }
            rec.auto_name = false;
        }
        inner.entries.insert(
            id,
            Entry {
                rec,
                segs: Arc::new(Mutex::new(Vec::new())),
                audio_segs: Arc::new(Mutex::new(Vec::new())),
                limiter: Arc::new(SpeedLimiter::new(0)),
                progress: Arc::new(Progress::default()),
                task: None,
                speed: SpeedMeter::default(),
                quota_prev: 0,
            },
        );
        inner.dirty = true;
        drop(inner);
        self.pump();
        id
    }

    /// Resume / start one download (it waits in the queue if too many run).
    pub fn start(self: &Arc<Self>, id: DownloadId) {
        {
            let mut inner = self.inner.lock().unwrap();
            if let Some(e) = inner.entries.get_mut(&id) {
                if matches!(e.rec.status, Status::Paused | Status::Failed) {
                    e.rec.status = Status::Queued;
                    e.rec.error = None;
                }
            }
            inner.dirty = true;
        }
        self.pump();
    }

    pub fn pause(&self, id: DownloadId) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(e) = inner.entries.get_mut(&id) {
            if e.rec.status.is_running() || e.rec.status == Status::Queued {
                e.rec.status = Status::Paused;
            }
            if let Some(t) = &e.task {
                t.cancel();
            }
        }
        inner.dirty = true;
    }

    pub fn start_all(self: &Arc<Self>) {
        {
            let mut inner = self.inner.lock().unwrap();
            for e in inner.entries.values_mut() {
                if matches!(e.rec.status, Status::Paused | Status::Failed) {
                    e.rec.status = Status::Queued;
                    e.rec.error = None;
                }
            }
            inner.dirty = true;
        }
        self.pump();
    }

    pub fn pause_all(&self) {
        let mut inner = self.inner.lock().unwrap();
        for e in inner.entries.values_mut() {
            if e.rec.status.is_running() || e.rec.status == Status::Queued {
                e.rec.status = Status::Paused;
            }
            if let Some(t) = &e.task {
                t.cancel();
            }
        }
        inner.dirty = true;
    }

    /// Remove from the list. Partial data is always deleted; the finished
    /// file only when `delete_file` is true.
    pub fn remove(&self, id: DownloadId, delete_file: bool) {
        let entry = {
            let mut inner = self.inner.lock().unwrap();
            inner.dirty = true;
            inner.entries.remove(&id)
        };
        let Some(entry) = entry else { return };
        if let Some(t) = &entry.task {
            t.cancel();
        }
        let rec = entry.rec;
        let data_dir = self.data_dir.clone();
        self.rt.spawn(async move {
            // give running connections a moment to close their file handles
            tokio::time::sleep(Duration::from_millis(800)).await;
            for _ in 0..5 {
                let mut ok = remove_temp_files(&rec);
                if delete_file && rec.status == Status::Completed && rec.kind == DownloadKind::Torrent {
                    ok &= remove_torrent_data(&rec);
                } else if delete_file && rec.status == Status::Completed && rec.target_path().exists() {
                    ok &= std::fs::remove_file(rec.target_path()).is_ok();
                }
                // our copy of its .torrent file
                if rec.kind == DownloadKind::Torrent && rec.url.ends_with(".torrent") && Path::new(&rec.url).starts_with(&data_dir) {
                    let _ = std::fs::remove_file(&rec.url);
                }
                if ok {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    /// Remove many downloads from the list at once (files stay on the disk,
    /// unfinished parts are cleaned up): one lock, one background clean-up.
    pub fn remove_many(&self, ids: &[DownloadId]) {
        let gone: Vec<Entry> = {
            let mut inner = self.inner.lock().unwrap();
            inner.dirty = true;
            ids.iter().filter_map(|id| inner.entries.remove(id)).collect()
        };
        if gone.is_empty() {
            return;
        }
        let mut recs = Vec::with_capacity(gone.len());
        for e in gone {
            if let Some(t) = &e.task {
                t.cancel();
            }
            if e.rec.status != Status::Completed {
                recs.push(e.rec);
            }
        }
        if recs.is_empty() {
            return;
        }
        self.rt.spawn(async move {
            tokio::time::sleep(Duration::from_millis(800)).await;
            for _ in 0..5 {
                let mut left = Vec::new();
                for rec in recs {
                    if !remove_temp_files(&rec) {
                        left.push(rec);
                    }
                }
                if left.is_empty() {
                    break;
                }
                recs = left;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    /// Take a download back completely: stop it and delete everything it
    /// wrote (parts, temporary files and the finished file), as if it had
    /// never started. Used when "Download File Info" is cancelled.
    pub fn discard(&self, id: DownloadId) {
        let entry = {
            let mut inner = self.inner.lock().unwrap();
            inner.dirty = true;
            inner.entries.remove(&id)
        };
        let Some(entry) = entry else { return };
        if let Some(t) = &entry.task {
            t.cancel();
        }
        let rec = entry.rec;
        self.rt.spawn(async move {
            tokio::time::sleep(Duration::from_millis(800)).await;
            // the file got its name when it was added (a free one), so
            // whatever is there now was written by this download
            for _ in 0..8 {
                let mut ok = remove_temp_files(&rec);
                let target = rec.target_path();
                if target.exists() {
                    ok &= std::fs::remove_file(&target).is_ok();
                }
                if ok {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    /// Move / rename a finished file ("Save as" for downloads that are done).
    pub fn move_file(&self, id: DownloadId, new_path: &Path) -> anyhow::Result<()> {
        // the list is not held while the file moves: a copy to another disk
        // can take minutes and everything else must go on meanwhile
        let from = {
            let inner = self.inner.lock().unwrap();
            let e = inner.entries.get(&id).ok_or_else(|| anyhow!("download not found"))?;
            if e.rec.status != Status::Completed {
                anyhow::bail!("only finished downloads can be moved");
            }
            e.rec.target_path()
        };
        if let Some(dir) = new_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if std::fs::rename(&from, new_path).is_err() {
            // another drive: copy, then delete the old one
            std::fs::copy(&from, new_path)?;
            std::fs::remove_file(&from)?;
        }
        let mut inner = self.inner.lock().unwrap();
        if let Some(e) = inner.entries.get_mut(&id) {
            e.rec.save_dir = new_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            if let Some(n) = new_path.file_name() {
                e.rec.file_name = n.to_string_lossy().into_owned();
            }
            e.rec.category = self.settings.read().unwrap().category_of(&e.rec.file_name, &e.rec.url);
        }
        inner.dirty = true;
        Ok(())
    }

    /// Put every download back into the category its file type says
    /// (after categories were added, changed or deleted).
    pub fn recategorize(&self) {
        let s = self.settings();
        let names = s.category_names();
        let mut inner = self.inner.lock().unwrap();
        for e in inner.entries.values_mut() {
            let wanted = s.category_of(&e.rec.file_name, &e.rec.url);
            if !names.contains(&e.rec.category) || wanted != "General" {
                e.rec.category = wanted;
            }
        }
        inner.dirty = true;
    }

    /// "Refresh download address": continue an unfinished download from a
    /// new link (the old one expired).
    pub fn set_url(self: &Arc<Self>, id: DownloadId, url: &str, headers: Option<RequestHeaders>) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(e) = inner.entries.get_mut(&id) {
            if let Some(t) = &e.task {
                t.cancel();
            }
            e.rec.url = url.to_string();
            e.rec.final_url = None;
            if let Some(h) = headers {
                let auth = e.rec.headers.auth.clone();
                e.rec.headers = h;
                if e.rec.headers.auth.is_none() {
                    e.rec.headers.auth = auth;
                }
            }
            if e.rec.status == Status::Failed {
                e.rec.status = Status::Paused;
                e.rec.error = None;
            }
        }
        inner.dirty = true;
    }

    /// Changes from the File Properties window.
    pub fn update_details(&self, id: DownloadId, d: DetailsEdit) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let e = inner.entries.get_mut(&id).ok_or_else(|| anyhow!("download not found"))?;
        if let Some(u) = d.url.filter(|u| !u.trim().is_empty() && *u != e.rec.url) {
            if e.rec.status == Status::Completed {
                anyhow::bail!("the address of a finished download can't be changed");
            }
            e.rec.url = u.trim().to_string();
            e.rec.final_url = None;
        }
        e.rec.headers.referer = d.referer.filter(|r| !r.trim().is_empty());
        e.rec.description = d.description.filter(|r| !r.trim().is_empty());
        e.rec.headers.auth = d.auth.filter(|(u, _)| !u.trim().is_empty());
        inner.dirty = true;
        Ok(())
    }

    /// Unfinished download: save to another folder / name (nothing written yet,
    /// or the temporary file moves along).
    pub fn change_target(&self, id: DownloadId, dir: &Path, name: &str) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let e = inner.entries.get_mut(&id).ok_or_else(|| anyhow!("download not found"))?;
        if e.task.is_some() {
            anyhow::bail!("stop the download first");
        }
        if e.rec.status == Status::Completed {
            anyhow::bail!("use Move / Rename for finished files");
        }
        let old = e.rec.clone();
        std::fs::create_dir_all(dir)?;
        e.rec.save_dir = dir.to_path_buf();
        e.rec.file_name = filename::sanitize(name);
        e.rec.auto_name = false;
        for (from, to) in old.temp_paths().into_iter().zip(e.rec.temp_paths()) {
            if from.exists() && from != to && std::fs::rename(&from, &to).is_err() {
                e.rec.save_dir = old.save_dir.clone();
                e.rec.file_name = old.file_name.clone();
                anyhow::bail!("could not move the partly downloaded data");
            }
        }
        inner.dirty = true;
        Ok(())
    }

    /// The "Download File Info" window was confirmed for a download that
    /// already started in the background: give it the final folder, name,
    /// note and login. Works while it downloads.
    pub fn retarget(
        &self,
        id: DownloadId,
        dir: &Path,
        name: &str,
        description: Option<String>,
        auth: Option<(String, String)>,
    ) -> anyhow::Result<()> {
        let done = {
            let mut inner = self.inner.lock().unwrap();
            let e = inner.entries.get_mut(&id).ok_or_else(|| anyhow!("download not found"))?;
            e.rec.description = description.filter(|d| !d.trim().is_empty());
            if auth.is_some() || e.rec.headers.auth.is_some() {
                e.rec.headers.auth = auth.filter(|(u, _)| !u.trim().is_empty());
            }
            let done = e.rec.status == Status::Completed;
            if !done {
                let name = filename::sanitize(name);
                if dir != e.rec.save_dir || name != e.rec.file_name {
                    std::fs::create_dir_all(dir)?;
                    // it may be downloading already (in the folder first
                    // chosen): its temporary files stay where they are, only
                    // the finished file goes to the new folder. Without this
                    // a resumed download wrote a new, nearly empty file there.
                    if e.rec.work_stem.is_some() && e.rec.temp_dir.as_ref().is_none_or(|t| t.as_os_str().is_empty()) {
                        e.rec.temp_dir = Some(e.rec.work_dir());
                    }
                    e.rec.save_dir = dir.to_path_buf();
                    e.rec.file_name = name;
                    e.rec.category = self.settings.read().unwrap().category_of(&e.rec.file_name, &e.rec.url);
                }
            }
            inner.dirty = true;
            done
        };
        // finished already (small file): move the file itself
        if done {
            let target = dir.join(filename::sanitize(name));
            let current = self.get(id).map(|v| v.save_path).unwrap_or_default();
            if target != current {
                self.move_file(id, &target)?;
            }
        }
        Ok(())
    }

    /// Files of a queue in their download order.
    pub fn queue_files(&self, queue: u64) -> Vec<DownloadView> {
        let inner = self.inner.lock().unwrap();
        let mut v: Vec<DownloadView> =
            inner.entries.values().filter(|e| e.rec.queue == Some(queue)).map(make_view).collect();
        v.sort_by_key(|x| (x.queue_pos, x.id));
        v
    }

    /// Move a file up (-1) or down (+1) inside its queue.
    pub fn move_in_queue(&self, id: DownloadId, delta: i32) {
        let mut inner = self.inner.lock().unwrap();
        let Some(q) = inner.entries.get(&id).and_then(|e| e.rec.queue) else { return };
        let mut order: Vec<(u64, DownloadId)> = inner
            .entries
            .values()
            .filter(|e| e.rec.queue == Some(q))
            .map(|e| (e.rec.queue_pos, e.rec.id))
            .collect();
        order.sort();
        let Some(i) = order.iter().position(|(_, x)| *x == id) else { return };
        let j = i as i64 + delta as i64;
        if j < 0 || j >= order.len() as i64 {
            return;
        }
        order.swap(i, j as usize);
        // renumber so positions are unique
        for (n, (_, x)) in order.iter().enumerate() {
            if let Some(e) = inner.entries.get_mut(x) {
                e.rec.queue_pos = n as u64 + 1;
            }
        }
        inner.dirty = true;
    }

    /// Sounds, messages: what happened since the last call.
    pub fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut self.inner.lock().unwrap().events)
    }

    /// Shut down / sleep / hibernate asked for (the app shows a countdown first).
    pub fn take_power_request(&self) -> Option<AfterDone> {
        self.inner.lock().unwrap().power_request.take()
    }

    /// Speed limit for one download (None = no limit). Takes effect at once.
    pub fn set_download_limit(&self, id: DownloadId, kb: Option<u64>) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(e) = inner.entries.get_mut(&id) {
            e.rec.speed_limit_kb = kb.filter(|k| *k > 0);
            e.limiter.set_rate(e.rec.speed_limit_kb.unwrap_or(0) * 1024);
        }
        inner.dirty = true;
    }

    /// What to do when this download finishes (shut down, sleep...).
    pub fn set_on_complete(&self, id: DownloadId, action: AfterDone) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(e) = inner.entries.get_mut(&id) {
            e.rec.on_complete = action;
        }
        inner.dirty = true;
    }

    /// What to do once every running and waiting download has finished.
    pub fn set_when_all_done(&self, action: AfterDone) {
        let mut inner = self.inner.lock().unwrap();
        inner.when_all_done = action;
        inner.all_done_seen_active = false;
    }

    pub fn when_all_done(&self) -> AfterDone {
        self.inner.lock().unwrap().when_all_done
    }

    /// Every address in the list (for "Export").
    pub fn all_urls(&self) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        inner
            .entries
            .values()
            .map(|e| e.rec.source_page.clone().unwrap_or_else(|| e.rec.url.clone()))
            .collect()
    }

    /// Is this address already in the list? Returns its id.
    pub fn find_url(&self, url: &str) -> Option<DownloadId> {
        let inner = self.inner.lock().unwrap();
        inner
            .entries
            .values()
            .find(|e| e.rec.url == url || e.rec.source_page.as_deref() == Some(url))
            .map(|e| e.rec.id)
    }

    pub fn remove_completed(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.entries.retain(|_, e| e.rec.status != Status::Completed);
        inner.dirty = true;
    }

    /// Start a failed/finished download again from zero.
    pub fn restart(self: &Arc<Self>, id: DownloadId) {
        {
            let mut inner = self.inner.lock().unwrap();
            let Some(e) = inner.entries.get_mut(&id) else { return };
            if e.task.is_some() {
                return;
            }
            remove_temp_files(&e.rec);
            e.segs.lock().unwrap().clear();
            e.audio_segs.lock().unwrap().clear();
            e.progress.bytes.store(0, Ordering::Relaxed);
            e.progress.parts_done.store(0, Ordering::Relaxed);
            e.rec.downloaded = 0;
            e.rec.parts_done = 0;
            e.rec.status = Status::Queued;
            e.rec.error = None;
            inner.dirty = true;
        }
        self.pump();
    }

    // ---------------------------------------------------------------- queries

    /// Every download, for the list (without the part tables: the list does
    /// not show them, and copying them several times a second costs).
    pub fn snapshot(&self) -> Vec<DownloadView> {
        let inner = self.inner.lock().unwrap();
        inner.entries.values().map(|e| view_of(e, false)).collect()
    }

    /// One download (cheap: used many times a second by progress windows).
    /// What can be watched / listened to of a download that is still
    /// running: the part of each file from its start with no gap (players
    /// stop at the first gap). `None`: nothing yet, or a kind that can't be
    /// played before it is done (streams joined at the end, torrents).
    pub fn preview_source(&self, id: DownloadId) -> Option<PreviewSource> {
        let inner = self.inner.lock().unwrap();
        let e = inner.entries.get(&id)?;
        if e.rec.status == Status::Completed {
            return None;
        }
        match e.rec.kind {
            DownloadKind::Http => {
                let segs = e.segs.lock().unwrap().clone();
                let ready = if segs.is_empty() { e.progress.bytes.load(Ordering::Relaxed) } else { ready_from_start(&segs) };
                let path = e.rec.part_path();
                (ready > 0 && path.exists()).then_some(PreviewSource::File { path, ready })
            }
            // picture and sound come as two files (YouTube): both are needed
            DownloadKind::Dash => {
                let (video, audio) = (e.rec.video_part_path(), e.rec.audio_part_path());
                let video_ready = ready_from_start(&e.segs.lock().unwrap());
                let audio_ready = ready_from_start(&e.audio_segs.lock().unwrap());
                (video_ready > 0 && video.exists()).then(|| PreviewSource::Pair {
                    video,
                    video_ready,
                    audio: (audio_ready > 0 && audio.exists()).then_some(audio),
                    audio_ready,
                })
            }
            DownloadKind::Hls | DownloadKind::Torrent => None,
        }
    }

    pub fn get(&self, id: DownloadId) -> Option<DownloadView> {
        self.inner.lock().unwrap().entries.get(&id).map(make_view)
    }

    /// Downloads that finished since the last call (for the "complete" popup).
    pub fn take_completed(&self) -> Vec<DownloadId> {
        std::mem::take(&mut self.inner.lock().unwrap().completed)
    }

    /// The scheduler asked the app to close.
    pub fn take_exit_request(&self) -> bool {
        std::mem::take(&mut self.inner.lock().unwrap().exit_requested)
    }

    /// Stop everything and write the state to disk (call before exiting).
    pub fn shutdown(&self) {
        let records = {
            let mut inner = self.inner.lock().unwrap();
            for e in inner.entries.values_mut() {
                if let Some(t) = &e.task {
                    t.cancel();
                }
                if e.rec.status.is_running() || e.rec.status == Status::Queued {
                    e.rec.status = Status::Paused;
                }
            }
            collect_records(&inner)
        };
        let _ = store::save_downloads(&self.data_dir.join("downloads.json"), &records);
    }

    // ---------------------------------------------------------------- internals

    /// Launch waiting downloads while there are free slots: no global limit
    /// unless set in options, and each queue runs its own number of files at
    /// a time (1 = one after another).
    fn pump(self: &Arc<Self>) {
        let settings = self.settings.read().unwrap().clone();
        let mut launch = Vec::new();
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.quota_blocked {
                return;
            }
            let mut running = inner.entries.values().filter(|e| e.task.is_some()).count();
            let mut per_queue: HashMap<u64, usize> = HashMap::new();
            for e in inner.entries.values().filter(|e| e.task.is_some()) {
                if let Some(q) = e.rec.queue {
                    *per_queue.entry(q).or_default() += 1;
                }
            }
            // queue files go in their queue order
            let mut order: Vec<(u64, DownloadId)> = inner
                .entries
                .values()
                .filter(|e| e.rec.status == Status::Queued && e.task.is_none())
                .map(|e| (if e.rec.queue.is_some() { e.rec.queue_pos } else { e.rec.id }, e.rec.id))
                .collect();
            order.sort();
            for (_, id) in order {
                let Some(e) = inner.entries.get_mut(&id) else { continue };
                if settings.max_active > 0 && running >= settings.max_active as usize {
                    break;
                }
                if e.rec.status != Status::Queued || e.task.is_some() {
                    continue;
                }
                if let Some(q) = e.rec.queue {
                    let limit = settings.queue(q).map(|d| d.max_parallel).unwrap_or(1);
                    let now = per_queue.get(&q).copied().unwrap_or(0);
                    if limit > 0 && now >= limit as usize {
                        continue;
                    }
                    *per_queue.entry(q).or_default() += 1;
                }
                let token = CancellationToken::new();
                e.task = Some(token.clone());
                e.rec.status = Status::Connecting;
                e.rec.last_try = Some(Utc::now());
                e.progress.assembling.store(false, Ordering::Relaxed);
                launch.push((id, token));
                running += 1;
            }
        }
        for (id, token) in launch {
            let m = self.clone();
            self.rt.spawn(async move { m.run_entry(id, token).await });
        }
    }

    // ---------------------------------------------------------------- queues

    /// Start every unfinished download in a queue (they run one after another,
    /// or as many at a time as the queue allows).
    pub fn start_queue(self: &Arc<Self>, queue: u64) {
        {
            let mut inner = self.inner.lock().unwrap();
            let mut any = false;
            for e in inner.entries.values_mut() {
                if e.rec.queue == Some(queue) && matches!(e.rec.status, Status::Paused | Status::Failed) {
                    e.rec.status = Status::Queued;
                    e.rec.error = None;
                    e.rec.retry_count = 0;
                }
                any |= e.rec.queue == Some(queue) && (e.rec.status == Status::Queued || e.task.is_some());
            }
            if any {
                inner.queue_armed.insert(queue);
                inner.events.push(Event::QueueStarted(queue));
            }
            inner.dirty = true;
        }
        self.pump();
    }

    /// Is any file of this queue running or waiting?
    pub fn queue_running(&self, queue: u64) -> bool {
        let inner = self.inner.lock().unwrap();
        inner
            .entries
            .values()
            .any(|e| e.rec.queue == Some(queue) && (e.task.is_some() || e.rec.status == Status::Queued))
    }

    pub fn stop_queue(&self, queue: u64) {
        let mut inner = self.inner.lock().unwrap();
        inner.queue_armed.remove(&queue);
        for e in inner.entries.values_mut() {
            if e.rec.queue != Some(queue) {
                continue;
            }
            if e.rec.status.is_running() || e.rec.status == Status::Queued {
                e.rec.status = Status::Paused;
            }
            if let Some(t) = &e.task {
                t.cancel();
            }
        }
        inner.dirty = true;
    }

    /// Put a download into a queue (None = take it out of any queue).
    pub fn set_queue(self: &Arc<Self>, id: DownloadId, queue: Option<u64>) {
        {
            let mut inner = self.inner.lock().unwrap();
            let last = inner
                .entries
                .values()
                .filter(|e| queue.is_some() && e.rec.queue == queue)
                .map(|e| e.rec.queue_pos)
                .max()
                .unwrap_or(0);
            if let Some(e) = inner.entries.get_mut(&id) {
                if e.rec.queue != queue {
                    e.rec.queue = queue;
                    // goes to the end of its new queue
                    e.rec.queue_pos = last.max(e.rec.id) + 1;
                }
            }
            inner.dirty = true;
        }
        self.pump();
    }

    /// Create a queue and return its id.
    pub fn add_queue(self: &Arc<Self>, name: &str, max_parallel: u32, folder: Option<PathBuf>) -> anyhow::Result<u64> {
        let mut s = self.settings();
        let id = s.queues.iter().map(|q| q.id).max().unwrap_or(MAIN_QUEUE) + 1;
        s.queues.push(QueueDef::new(id, name.trim(), max_parallel, folder));
        self.set_settings(s)?;
        Ok(id)
    }

    pub fn update_queue(self: &Arc<Self>, def: QueueDef) -> anyhow::Result<()> {
        let mut s = self.settings();
        if let Some(q) = s.queues.iter_mut().find(|q| q.id == def.id) {
            *q = def;
        }
        self.set_settings(s)
    }

    /// Delete a queue; its downloads move to the main queue.
    pub fn delete_queue(self: &Arc<Self>, id: u64) -> anyhow::Result<()> {
        if id == MAIN_QUEUE {
            anyhow::bail!("the main queue can't be deleted");
        }
        {
            let mut inner = self.inner.lock().unwrap();
            for e in inner.entries.values_mut() {
                if e.rec.queue == Some(id) {
                    e.rec.queue = Some(MAIN_QUEUE);
                }
            }
            inner.dirty = true;
        }
        let mut s = self.settings();
        s.queues.retain(|q| q.id != id);
        self.set_settings(s)
    }

    async fn run_entry(self: Arc<Self>, id: DownloadId, cancel: CancellationToken) {
        let progress = self.inner.lock().unwrap().entries.get(&id).map(|e| e.progress.clone());
        let mut tries = 0u32;
        // server trouble: tries without anything new downloaded in between
        let mut stuck = 0u32;
        // new links fetched because the old one stopped working
        let mut relinks = 0u32;
        let mut last_bytes = progress.as_ref().map(|p| p.bytes.load(Ordering::Relaxed)).unwrap_or(0);
        let result = loop {
            let r = self.run_download(id, cancel.clone()).await;
            match (&r, &progress) {
                // no internet when starting (or the whole transfer broke off):
                // wait in place and try again until it is back or the user stops it
                (Err(e), Some(p)) if !cancel.is_cancelled() && !e.is::<Cancelled>() && http::is_network_error(e) => {
                    tries += 1;
                    p.waiting.store(true, Ordering::Relaxed);
                    tokio::select! {
                        _ = tokio::time::sleep(http::reconnect_delay(tries)) => {}
                        _ = cancel.cancelled() => break Err(Cancelled.into()),
                    }
                }
                // a busy server, a transfer that broke off (big files): it
                // goes on by itself later, from where it was. Given up only
                // when many tries in a row brought nothing new (hours).
                // the link stopped working (expired): a new one, checked before
                // anything more is written; given up after 6 tries
                // (a download that got nothing yet: 2 tries, it may never have worked)
                (Err(e), Some(p))
                    if !cancel.is_cancelled()
                        && !e.is::<Cancelled>()
                        && http::is_link_gone(e)
                        && relinks < if p.bytes.load(Ordering::Relaxed) > 0 { 6 } else { 2 } =>
                {
                    relinks += 1;
                    self.relink.lock().unwrap().insert(id);
                    let wait = http::relink_delay(relinks);
                    let note = format!("the link stopped working - getting a new one in {} s (try {relinks})", wait.as_secs());
                    self.update(id, move |x| x.error = Some(note));
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = cancel.cancelled() => break Err(Cancelled.into()),
                    }
                }
                (Err(e), Some(p)) if !cancel.is_cancelled() && !e.is::<Cancelled>() && http::is_passing_error(e) && self.resumable(id) => {
                    let now = p.bytes.load(Ordering::Relaxed);
                    stuck = if now > last_bytes { 1 } else { stuck + 1 };
                    last_bytes = now;
                    if stuck > 12 {
                        break r;
                    }
                    let wait = http::retry_delay(stuck);
                    let note = format!("{e:#} - trying again in {} s", wait.as_secs());
                    self.update(id, move |x| x.error = Some(note));
                    tokio::select! {
                        _ = tokio::time::sleep(wait) => {}
                        _ = cancel.cancelled() => break Err(Cancelled.into()),
                    }
                }
                _ => break r,
            }
        };
        if let Some(p) = &progress {
            p.waiting.store(false, Ordering::Relaxed);
        }
        self.relink.lock().unwrap().remove(&id);
        {
            let mut inner = self.inner.lock().unwrap();
            let mut finished = false;
            let mut failed = false;
            let mut open_file = false;
            let mut mark: Option<(PathBuf, String, Option<String>)> = None;
            let mut action = AfterDone::Nothing;
            let settings = self.settings();
            if let Some(e) = inner.entries.get_mut(&id) {
                e.task = None;
                e.rec.segments = e.segs.lock().unwrap().clone();
                e.rec.audio_segments = e.audio_segs.lock().unwrap().clone();
                e.rec.downloaded = e.progress.bytes.load(Ordering::Relaxed);
                e.rec.parts_total = e.progress.parts_total.load(Ordering::Relaxed);
                e.rec.parts_done = e.progress.parts_done.load(Ordering::Relaxed);
                e.progress.assembling.store(false, Ordering::Relaxed);
                match result {
                    Ok(()) => {
                        e.rec.status = Status::Completed;
                        e.rec.error = None;
                        e.rec.finished_at = Some(Utc::now());
                        e.rec.segments.clear();
                        e.rec.audio_segments.clear();
                        if e.rec.total_size.is_none() {
                            e.rec.total_size = Some(e.rec.downloaded);
                        }
                        e.segs.lock().unwrap().clear();
                        e.audio_segs.lock().unwrap().clear();
                        finished = true;
                        mark = Some((
                            e.rec.target_path(),
                            e.rec.final_url.clone().unwrap_or_else(|| e.rec.url.clone()),
                            e.rec.headers.referer.clone(),
                        ));
                        action = std::mem::take(&mut e.rec.on_complete);
                        open_file = e.rec.queue.and_then(|q| settings.queue(q)).map(|q| q.open_when_done).unwrap_or(false);
                    }
                    Err(err) if cancel.is_cancelled() || err.is::<Cancelled>() => {
                        if e.rec.status.is_running() {
                            e.rec.status = Status::Paused;
                        }
                    }
                    Err(err) => {
                        // the queue tries a failed file again a few times
                        let retries = e.rec.queue.and_then(|q| settings.queue(q)).map(|q| q.retries).unwrap_or(0);
                        if e.rec.retry_count < retries {
                            e.rec.retry_count += 1;
                            e.rec.status = Status::Queued;
                            e.rec.error = Some(format!("{err:#} (trying again)"));
                        } else {
                            e.rec.status = Status::Failed;
                            e.rec.error = Some(format!("{err:#}"));
                            failed = true;
                        }
                    }
                }
            }
            if finished && settings.subtitles && !settings.subtitle_langs.trim().is_empty() {
                // a site's video: its subtitles next to it (when there are any)
                if let Some(e) = inner.entries.get(&id) {
                    if let (Some(page), DownloadKind::Dash | DownloadKind::Http | DownloadKind::Hls) = (e.rec.source_page.clone(), e.rec.kind) {
                        let (dir, langs) = (e.rec.save_dir.clone(), settings.subtitle_langs.clone());
                        let (stem, _) = filename::split_ext(&e.rec.file_name);
                        let stem = stem.to_string();
                        if !matches!(filename::extension(&e.rec.file_name).as_str(), "mp3" | "m4a" | "opus" | "ogg") {
                            self.rt.spawn_blocking(move || {
                                let _ = ytdlp::fetch_subtitles(&page, &dir, &stem, &langs);
                            });
                        }
                    }
                }
            }
            if finished {
                // before anything can open it: the file comes from the internet
                if let Some((path, host, referrer)) = &mark {
                    system::mark_from_internet(path, host, referrer.as_deref());
                }
                inner.completed.push(id);
                inner.events.push(Event::Completed(id));
                if open_file {
                    inner.events.push(Event::OpenFile(id));
                }
            }
            if failed {
                inner.events.push(Event::Failed(id));
            }
            inner.dirty = true;
            drop(inner);
            if action != AfterDone::Nothing {
                self.after_done(action);
            }
        }
        self.pump();
    }

    /// It can go on from where it stopped (a later try keeps what it has).
    fn resumable(&self, id: DownloadId) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.entries.get(&id).map(|e| e.rec.resumable || e.rec.kind != DownloadKind::Http).unwrap_or(false)
    }

    fn update(&self, id: DownloadId, f: impl FnOnce(&mut DownloadRecord)) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(e) = inner.entries.get_mut(&id) {
            f(&mut e.rec);
        }
        inner.dirty = true;
    }

    async fn run_download(self: &Arc<Self>, id: DownloadId, cancel: CancellationToken) -> anyhow::Result<()> {
        let settings = self.settings();
        let (mut rec, segs, audio_segs, progress, limiter) = {
            let inner = self.inner.lock().unwrap();
            let e = inner.entries.get(&id).ok_or_else(|| anyhow!("download was removed"))?;
            (e.rec.clone(), e.segs.clone(), e.audio_segs.clone(), e.progress.clone(), e.limiter.clone())
        };
        if let Some(q) = rec.pick_quality.clone() {
            self.resolve_pick(id, &mut rec, &q, &cancel).await?;
        }
        rec.headers = with_login(&settings, &rec.url, &rec.headers);
        rec.connections = settings.connections_for(&rec.url, rec.connections);
        if rec.kind == DownloadKind::Dash {
            return self.run_dash(id, rec, segs, audio_segs, progress, limiter, cancel, &settings).await;
        }
        if rec.kind == DownloadKind::Torrent {
            return self.run_torrent(id, rec, progress, cancel, &settings).await;
        }
        self.refresh_source(id, &mut rec, &cancel).await?;

        // Fresh look at the server every time: links behind redirects often
        // expire, and the file may have changed since we paused.
        let client = self.shared.client();
        let p = tokio::select! {
            r = http::probe(&client, &rec.url, &rec.headers, &settings.user_agent) => r?,
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        if p.is_web_page() && rec.downloaded == 0 && segs.lock().unwrap().is_empty() {
            anyhow::bail!(
                "the address opens a web page, not a file (the link may have expired - use \"Refresh download address\")"
            );
        }
        // a new link after the old one stopped working: only the same file
        // (same size, parts can be asked for) goes on from where it was
        if self.relink.lock().unwrap().contains(&id) && !segs.lock().unwrap().is_empty() {
            same_file(&p, rec.total_size)?;
            self.relink.lock().unwrap().remove(&id);
        }
        {
            let mut table = segs.lock().unwrap();
            let changed = rec.total_size.is_some() && p.total_size != rec.total_size;
            if changed || !p.resumable {
                table.clear();
            }
        }
        // the server only took the request without the browser's cookies /
        // referrer: the download asks the same way
        if let Some(h) = &p.headers_used {
            let auth = rec.headers.auth.clone();
            rec.headers = h.clone();
            rec.headers.auth = auth;
        }
        rec.final_url = Some(p.final_url.clone());
        rec.total_size = p.total_size;
        rec.resumable = p.resumable;
        rec.kind = p.kind;
        rec.server_time = p.last_modified.or(rec.server_time);
        rec.connections = settings.connections_for(&p.final_url, rec.connections);
        // where it was to go when this start began (the user may choose
        // another name / folder meanwhile, in "Download File Info")
        let (was_dir, was_name) = (rec.save_dir.clone(), rec.file_name.clone());
        // added without a name (batch, clipboard): use the server's real name
        // and the matching category folder before the first byte is written
        if rec.auto_name && segs.lock().unwrap().is_empty() && rec.downloaded == 0 && p.file_name != rec.file_name {
            let new_cat = settings.category_of(&p.file_name, &rec.url);
            let dir = if rec.save_dir == settings.dir_for(&rec.category) {
                settings.dir_for(&new_cat)
            } else {
                rec.save_dir.clone()
            };
            let _ = std::fs::create_dir_all(&dir);
            let name = free_name(&self.inner.lock().unwrap(), &dir, &p.file_name);
            rec.file_name = name;
            rec.save_dir = dir;
            rec.category = new_cat;
        }
        rec.auto_name = false;
        {
            let r = rec.clone();
            self.update(id, move |x| {
                x.final_url = r.final_url;
                x.total_size = r.total_size;
                x.resumable = r.resumable;
                x.kind = r.kind;
                // only when nobody changed them in the meantime
                if x.save_dir == was_dir && x.file_name == was_name {
                    x.file_name = r.file_name;
                    x.save_dir = r.save_dir;
                    x.category = r.category;
                }
                x.auto_name = false;
                x.server_time = r.server_time;
                if x.status == Status::Connecting {
                    x.status = Status::Downloading;
                }
            });
        }
        if let Some(total) = rec.total_size {
            let have = segments::downloaded(&segs.lock().unwrap());
            system::check_room(&rec.work_dir(), total.saturating_sub(have), Some(total)).map_err(|m| anyhow!(m))?;
            if rec.work_dir() != rec.save_dir {
                // the finished file is copied over to the other disk
                system::check_room(&rec.save_dir, total, Some(total)).map_err(|m| anyhow!(m))?;
            }
        }

        let part = rec.part_path();
        match rec.kind {
            DownloadKind::Http => {
                let job = HttpJob {
                    url: rec.request_url().to_string(),
                    headers: rec.headers.clone(),
                    user_agent: settings.user_agent.clone(),
                    total_size: rec.total_size,
                    resumable: rec.resumable,
                    part_path: part.clone(),
                    connections: rec.connections,
                    min_split: settings.min_split(),
                    retries: settings.retries,
                    progress_base: 0,
                    max_request: http_download::max_request_for(rec.request_url()),
                    limiter: limiter.clone(),
                };
                http_download::run(self.shared.clone(), job, segs, progress.clone(), cancel).await?;
            }
            DownloadKind::Hls => {
                let job = HlsJob {
                    url: rec.request_url().to_string(),
                    headers: rec.headers.clone(),
                    user_agent: settings.user_agent.clone(),
                    output: part.clone(),
                    temp_dir: rec.hls_temp_dir(),
                    connections: rec.connections,
                    retries: settings.retries,
                    limiter: limiter.clone(),
                };
                hls::run(self.shared.clone(), job, progress.clone(), cancel).await?;
            }
            DownloadKind::Dash => unreachable!("handled by run_dash"),
            DownloadKind::Torrent => unreachable!("handled by run_torrent"),
        }
        if rec.source_key.as_deref() == Some(ytdlp::MP3_KEY) {
            // "Audio · MP3": the sound is here, now make the MP3 file
            progress.assembling.store(true, Ordering::Relaxed);
            let (input, tmp) = (part.clone(), part.with_extension("mp3tmp"));
            let t2 = tmp.clone();
            let title = song_title(&rec.file_name);
            let kbps = ytdlp::mp3_kbps();
            tokio::task::spawn_blocking(move || mux::to_mp3(&input, &t2, kbps, Some(&title))).await??;
            std::fs::rename(&tmp, &part)?;
        }
        if rec.kind == DownloadKind::Hls && filename::extension(&rec.file_name) == "ts" && mux::find_ffmpeg().is_some() {
            // a joined stream: an ordinary MP4 plays everywhere and can be
            // wound forward; if that fails the .ts file stays as it is
            progress.assembling.store(true, Ordering::Relaxed);
            let (input, tmp) = (part.clone(), part.with_extension("mp4tmp"));
            let t2 = tmp.clone();
            let made = tokio::task::spawn_blocking(move || mux::ts_to_mp4(&input, &t2)).await.map(|r| r.is_ok()).unwrap_or(false);
            if made && std::fs::rename(&tmp, &part).is_ok() {
                let (stem, _) = filename::split_ext(&rec.file_name);
                let name = filename::unique_name(&rec.save_dir, &format!("{stem}.mp4"));
                self.update(id, move |x| x.file_name = name);
            } else {
                let _ = std::fs::remove_file(&tmp);
            }
        }
        self.finish_file(id, &rec)
    }

    /// A torrent: aria2 gets it, we watch and show its progress.
    async fn run_torrent(
        self: &Arc<Self>,
        id: DownloadId,
        rec: DownloadRecord,
        progress: Arc<Progress>,
        cancel: CancellationToken,
        settings: &Settings,
    ) -> anyhow::Result<()> {
        let _ = std::fs::create_dir_all(&rec.save_dir);
        let target = rec.target_path();
        let control = rec.save_dir.join(format!("{}.aria2", rec.file_name));
        let limit_kb = rec.speed_limit_kb.unwrap_or(settings.effective_limit() / 1024);
        let opts = torrent::AddOptions {
            dir: rec.save_dir.clone(),
            select: rec.torrent_select.clone(),
            seed_minutes: settings.torrent_seed_minutes,
            download_limit_kb: limit_kb,
            upload_limit_kb: settings.torrent_upload_kb,
            // all there already (no notes of aria2 about it): check it
            check: target.exists() && !control.exists(),
        };
        // files that were there before: never removed by us
        let existed: HashSet<PathBuf> = rec.torrent_data_paths().into_iter().filter(|p| p.exists()).collect();
        let mut gid = tokio::select! {
            r = self.torrents.add(&rec.url, &opts) => r?,
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        self.update(id, |x| {
            if x.status == Status::Connecting {
                x.status = Status::Downloading;
            }
        });
        let mut named = false;
        let mut listed = !rec.torrent_files.is_empty();
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(800)) => {}
                _ = cancel.cancelled() => {
                    self.torrents.remove(&gid).await;
                    return Err(Cancelled.into());
                }
            }
            let st = match self.torrents.status(&gid).await {
                Ok(st) => st,
                Err(e) => {
                    self.torrents.remove(&gid).await;
                    return Err(e.context("the torrent program stopped"));
                }
            };
            // a magnet link found its file list: the torrent goes on under a new id
            if let Some(next) = st.followed_by.clone() {
                gid = next;
                continue;
            }
            // the chosen files only: aria2's own total also counts the
            // pieces they share with files that were left out
            let files = if st.total > 0 { self.torrents.files(&gid).await.unwrap_or_default() } else { Vec::new() };
            let chosen: Vec<&torrent::FileState> = files.iter().filter(|f| f.selected).collect();
            let (total, done) = if chosen.is_empty() {
                (st.total, st.done)
            } else {
                (chosen.iter().map(|f| f.size).sum(), chosen.iter().map(|f| f.done.min(f.size)).sum())
            };
            progress.bytes.store(done, Ordering::Relaxed);
            progress.connections.store(st.connections, Ordering::Relaxed);
            if !listed && !chosen.is_empty() {
                // the files it writes, to clean up exactly those later
                let rel: Vec<String> = chosen
                    .iter()
                    .filter_map(|f| f.path.strip_prefix(&rec.save_dir).ok().map(|r| r.to_string_lossy().into_owned()))
                    .filter(|r| !r.is_empty())
                    .collect();
                if !rel.is_empty() {
                    listed = true;
                    self.update(id, move |x| x.torrent_files = rel);
                }
            }
            if total > 0 {
                let name = if named { None } else { st.name.clone().map(|n| filename::sanitize(&n)) };
                named = named || name.is_some();
                self.update(id, move |x| {
                    x.total_size = Some(total);
                    x.resumable = true;
                    if let Some(n) = name {
                        x.file_name = n;
                    }
                });
            }
            let finished = match st.status.as_str() {
                "complete" => true,
                "error" => {
                    let msg = st.error.unwrap_or_else(|| "the torrent could not be downloaded".into());
                    self.torrents.remove(&gid).await;
                    anyhow::bail!("{msg}");
                }
                "removed" => anyhow::bail!("the torrent was stopped"),
                // everything is here and it shares for a while: done for us
                _ => st.seeding && total > 0 && done >= total,
            };
            if finished {
                // no sharing wanted (or its time is over): aria2 forgets it,
                // so the same torrent can be added again
                if st.status == "complete" || settings.torrent_seed_minutes == 0 {
                    self.torrents.remove(&gid).await;
                }
                // the bits of the files that were left out (pieces are shared
                // between files): not wanted, and not the user's own
                for f in files.iter().filter(|f| !f.selected) {
                    if inside(&f.path, &rec.save_dir) && f.path.is_file() && !existed.contains(&f.path) {
                        let _ = std::fs::remove_file(&f.path);
                        if let Some(dir) = f.path.parent() {
                            let _ = std::fs::remove_dir(dir); // only when empty
                        }
                    }
                }
                return Ok(());
            }
        }
    }

    /// A playlist video added as its page: ask yt-dlp for its links now and
    /// take the wanted quality.
    async fn resolve_pick(
        &self,
        id: DownloadId,
        rec: &mut DownloadRecord,
        quality: &str,
        cancel: &CancellationToken,
    ) -> anyhow::Result<()> {
        let page = rec.source_page.clone().unwrap_or_else(|| rec.url.clone());
        let p2 = page.clone();
        let job = tokio::task::spawn_blocking(move || ytdlp::resolve(&p2, None));
        let info = tokio::select! {
            r = job => r??,
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        let opt = ytdlp::pick_option(&info.options, quality)
            .ok_or_else(|| anyhow!("no downloadable video was found on this page"))?;
        rec.url = opt.video_url.clone();
        rec.final_url = None;
        rec.audio_url = opt.audio_url.clone();
        rec.kind = if opt.audio_url.is_some() { DownloadKind::Dash } else { DownloadKind::Http };
        rec.source_page = Some(page.clone());
        rec.source_key = Some(opt.key.clone());
        ytdlp::apply_headers(&mut rec.headers, &opt.headers);
        // The video's own title (yt-dlp gives the title in the video's own
        // language here; the playlist page often shows translated ones),
        // keeping the "07 - " number of the playlist; the real container
        // decides the extension.
        let (stem, ext) = filename::split_ext(&rec.file_name);
        let stem = playlist_stem(stem, &info.title);
        if !ext.eq_ignore_ascii_case(&opt.ext) || stem != filename::split_ext(&rec.file_name).0 {
            let wanted = filename::sanitize(&format!("{stem}.{}", opt.ext));
            rec.file_name = free_name(&self.inner.lock().unwrap(), &rec.save_dir, &wanted);
        }
        rec.pick_quality = None;
        rec.category = self.settings.read().unwrap().category_of(&rec.file_name, &page);
        let r = rec.clone();
        self.update(id, move |x| {
            x.url = r.url;
            x.final_url = None;
            x.audio_url = r.audio_url;
            x.kind = r.kind;
            x.source_page = r.source_page;
            x.source_key = r.source_key;
            x.headers = r.headers;
            x.file_name = r.file_name;
            x.category = r.category;
            x.pick_quality = None;
        });
        Ok(())
    }

    /// YouTube links expire after a few hours. When this download came from a
    /// YouTube page and is being resumed (or waited a while), ask yt-dlp for
    /// fresh links of the same quality. Failure is not fatal: we try the old ones.
    async fn refresh_source(
        &self,
        id: DownloadId,
        rec: &mut DownloadRecord,
        cancel: &CancellationToken,
    ) -> anyhow::Result<()> {
        let (Some(page), Some(key)) = (rec.source_page.clone(), rec.source_key.clone()) else {
            return Ok(());
        };
        let started = !rec.segments.is_empty() || rec.downloaded > 0;
        let old = Utc::now().signed_duration_since(rec.added_at) > chrono::Duration::minutes(30);
        if !started && !old {
            return Ok(());
        }
        let job = tokio::task::spawn_blocking(move || ytdlp::resolve_key(&page, &key));
        let fresh = tokio::select! {
            r = job => r,
            _ = cancel.cancelled() => return Err(Cancelled.into()),
        };
        if let Ok(Ok(opt)) = fresh {
            rec.url = opt.video_url.clone();
            rec.final_url = None;
            rec.audio_url = opt.audio_url.clone();
            ytdlp::apply_headers(&mut rec.headers, &opt.headers);
            let r = rec.clone();
            self.update(id, move |x| {
                x.url = r.url;
                x.final_url = None;
                x.audio_url = r.audio_url;
                x.headers = r.headers;
            });
        }
        Ok(())
    }

    /// `.sgpart` -> real name (pick a new name if that one got taken meanwhile)
    fn finish_file(&self, id: DownloadId, rec: &DownloadRecord) -> anyhow::Result<()> {
        // name and folder may have changed while it was downloading
        let (save_dir, file_name) = {
            let inner = self.inner.lock().unwrap();
            inner
                .entries
                .get(&id)
                .map(|e| (e.rec.save_dir.clone(), e.rec.file_name.clone()))
                .unwrap_or_else(|| (rec.save_dir.clone(), rec.file_name.clone()))
        };
        let mut target = save_dir.join(&file_name);
        if target.exists() {
            let name = filename::unique_name(&save_dir, &file_name);
            target = save_dir.join(&name);
            self.update(id, move |x| x.file_name = name);
        }
        let _ = std::fs::create_dir_all(&save_dir);
        if std::fs::rename(rec.part_path(), &target).is_err() {
            // temporary folder on another disk: copy, then remove
            std::fs::copy(rec.part_path(), &target)?;
            let _ = std::fs::remove_file(rec.part_path());
        }
        let settings = self.settings();
        if settings.set_file_date {
            if let Some(t) = rec.server_time {
                system::set_file_time(&target, t);
            }
        }
        if settings.virus_scan {
            system::virus_scan(&settings.virus_program, &settings.virus_args, &target);
        }
        Ok(())
    }

    /// YouTube style download: video-only + audio-only streams, each with
    /// many connections, then merged by ffmpeg into one file.
    #[allow(clippy::too_many_arguments)]
    async fn run_dash(
        self: &Arc<Self>,
        id: DownloadId,
        mut rec: DownloadRecord,
        segs: SegTable,
        audio_segs: SegTable,
        progress: Arc<Progress>,
        limiter: Arc<SpeedLimiter>,
        cancel: CancellationToken,
        settings: &Settings,
    ) -> anyhow::Result<()> {
        let audio_url = rec
            .audio_url
            .clone()
            .ok_or_else(|| anyhow!("this download has no audio stream"))?;
        let finished = |t: &SegTable| {
            let t = t.lock().unwrap();
            !t.is_empty() && segments::all_finished(&t)
        };
        // both parts already on disk (e.g. ffmpeg was missing last time):
        // skip the network completely, the links may have expired by now
        let both_done = finished(&segs)
            && finished(&audio_segs)
            && rec.video_part_path().exists()
            && rec.audio_part_path().exists();

        if !both_done {
            self.refresh_source(id, &mut rec, &cancel).await?;
            let audio_url = rec.audio_url.clone().unwrap_or(audio_url);
            let ua = settings.user_agent.clone();
            let client = self.shared.client();
            let probes = async {
                tokio::try_join!(
                    http::probe(&client, &rec.url, &rec.headers, &ua),
                    http::probe(&client, &audio_url, &rec.headers, &ua),
                )
            };
            let (vp, ap) = tokio::select! {
                r = probes => r?,
                _ = cancel.cancelled() => return Err(Cancelled.into()),
            };
            let old_audio = rec.total_size.zip(rec.video_size).map(|(t, v)| t.saturating_sub(v));
            if self.relink.lock().unwrap().contains(&id) {
                if !segs.lock().unwrap().is_empty() {
                    same_file(&vp, rec.video_size)?;
                }
                if !audio_segs.lock().unwrap().is_empty() {
                    same_file(&ap, old_audio)?;
                }
                self.relink.lock().unwrap().remove(&id);
            }
            if vp.total_size != rec.video_size || !vp.resumable {
                segs.lock().unwrap().clear();
            }
            if ap.total_size != old_audio || !ap.resumable {
                audio_segs.lock().unwrap().clear();
            }
            rec.video_size = vp.total_size;
            rec.total_size = vp.total_size.zip(ap.total_size).map(|(v, a)| v + a);
            rec.resumable = vp.resumable && ap.resumable;
            rec.final_url = Some(vp.final_url.clone());
            if let Some(total) = rec.total_size {
                let have = segments::downloaded(&segs.lock().unwrap()) + segments::downloaded(&audio_segs.lock().unwrap());
                // video + audio parts, then the merged file next to them
                system::check_room(&rec.work_dir(), (total * 2).saturating_sub(have), Some(total)).map_err(|m| anyhow!(m))?;
            }
            {
                let r = rec.clone();
                self.update(id, move |x| {
                    x.final_url = r.final_url;
                    x.total_size = r.total_size;
                    x.video_size = r.video_size;
                    x.resumable = r.resumable;
                    if x.status == Status::Connecting {
                        x.status = Status::Downloading;
                    }
                });
            }

            let video_job = HttpJob {
                url: vp.final_url.clone(),
                headers: rec.headers.clone(),
                user_agent: ua.clone(),
                total_size: vp.total_size,
                resumable: vp.resumable,
                part_path: rec.video_part_path(),
                connections: rec.connections,
                min_split: settings.min_split(),
                retries: settings.retries,
                progress_base: 0,
                max_request: http_download::max_request_for(&vp.final_url),
                limiter: limiter.clone(),
            };
            http_download::run(self.shared.clone(), video_job.clone(), segs.clone(), progress.clone(), cancel.clone())
                .await?;
            let video_bytes = vp.total_size.unwrap_or_else(|| progress.bytes.load(Ordering::Relaxed));
            let audio_job = HttpJob {
                url: ap.final_url.clone(),
                total_size: ap.total_size,
                resumable: ap.resumable,
                part_path: rec.audio_part_path(),
                progress_base: video_bytes,
                max_request: http_download::max_request_for(&ap.final_url),
                ..video_job
            };
            http_download::run(self.shared.clone(), audio_job, audio_segs.clone(), progress.clone(), cancel.clone())
                .await?;
        } else {
            self.update(id, |x| {
                if x.status == Status::Connecting {
                    x.status = Status::Downloading;
                }
            });
        }

        // merge
        progress.assembling.store(true, Ordering::Relaxed);
        let (video, audio, output, name) = (
            rec.video_part_path(),
            rec.audio_part_path(),
            rec.part_path(),
            rec.file_name.clone(),
        );
        tokio::task::spawn_blocking(move || mux::merge(&video, &audio, &output, &name)).await??;
        let _ = std::fs::remove_file(rec.video_part_path());
        let _ = std::fs::remove_file(rec.audio_part_path());
        self.finish_file(id, &rec)
    }

    fn tick(self: &Arc<Self>) {
        let to_save = {
            let mut inner = self.inner.lock().unwrap();
            let now = Instant::now();
            let mut any_running = false;
            let mut fresh_bytes = 0u64;
            for e in inner.entries.values_mut() {
                let bytes = e.progress.bytes.load(Ordering::Relaxed);
                if e.task.is_some() && bytes > e.quota_prev {
                    fresh_bytes += bytes - e.quota_prev;
                }
                e.quota_prev = bytes;
                let m = &mut e.speed;
                if e.task.is_none() {
                    m.samples.clear();
                    m.value = 0.0;
                    continue;
                }
                any_running = true;
                // a restart/resume can make the counter jump back: start over
                if m.samples.back().map(|(_, b)| bytes < *b).unwrap_or(false) {
                    m.samples.clear();
                }
                m.samples.push_back((now, bytes));
                while m.samples.len() > 2 && now.duration_since(m.samples[0].0) > Duration::from_millis(2000) {
                    m.samples.pop_front();
                }
                if let (Some(first), Some(last)) = (m.samples.front(), m.samples.back()) {
                    let dt = last.0.duration_since(first.0).as_secs_f64();
                    if dt > 0.15 {
                        let rate = (last.1 - first.1) as f64 / dt;
                        // light smoothing on top of the 2 s window
                        m.value = if m.value == 0.0 { rate } else { m.value * 0.5 + rate * 0.5 };
                    }
                }
            }
            self.track_quota(&mut inner, now, fresh_bytes);
            // changes to the list: after 3 s; only progress of running
            // downloads: every 10 s (resume works from there anyway)
            let since = now.duration_since(inner.last_save);
            let due = (inner.dirty && since > Duration::from_secs(3)) || (any_running && since > Duration::from_secs(10));
            if due {
                inner.dirty = false;
                inner.last_save = now;
                Some(collect_records(&inner))
            } else {
                None
            }
        };
        if let Some(records) = to_save {
            let _ = store::save_downloads(&self.data_dir.join("downloads.json"), &records);
        }
        self.pump();
        self.check_schedule();
        self.check_all_done();
    }

    /// Options > Connection > "Download no more than X MB every Y hours".
    fn track_quota(&self, inner: &mut Inner, now: Instant, fresh: u64) {
        let s = self.settings.read().unwrap();
        if !s.quota_on || s.quota_mb == 0 {
            inner.quota_log.clear();
            if inner.quota_blocked {
                inner.quota_blocked = false;
            }
            return;
        }
        let window = Duration::from_secs(s.quota_hours.max(1) as u64 * 3600);
        let limit = s.quota_mb * 1024 * 1024;
        drop(s);
        if fresh > 0 {
            match inner.quota_log.back_mut() {
                // one entry per minute is enough
                Some((t, b)) if now.duration_since(*t) < Duration::from_secs(60) => *b += fresh,
                _ => inner.quota_log.push_back((now, fresh)),
            }
        }
        while inner.quota_log.front().map(|(t, _)| now.duration_since(*t) > window).unwrap_or(false) {
            inner.quota_log.pop_front();
        }
        let used: u64 = inner.quota_log.iter().map(|(_, b)| *b).sum();
        if used >= limit && !inner.quota_blocked {
            inner.quota_blocked = true;
            let mut stopped = Vec::new();
            for e in inner.entries.values_mut() {
                if e.task.is_some() || e.rec.status == Status::Queued {
                    if let Some(t) = &e.task {
                        t.cancel();
                    }
                    e.rec.status = Status::Paused;
                    e.rec.error = Some("download limit reached (Options > Connection)".into());
                    stopped.push(e.rec.id);
                }
            }
            inner.quota_paused.extend(stopped);
            inner.events.push(Event::QuotaReached);
            inner.dirty = true;
        } else if used < limit && inner.quota_blocked {
            inner.quota_blocked = false;
            for id in std::mem::take(&mut inner.quota_paused) {
                if let Some(e) = inner.entries.get_mut(&id) {
                    if e.rec.status == Status::Paused {
                        e.rec.status = Status::Queued;
                        e.rec.error = None;
                    }
                }
            }
            inner.dirty = true;
        }
    }

    /// Bytes downloaded inside the current quota window (for Options).
    pub fn quota_used(&self) -> u64 {
        self.inner.lock().unwrap().quota_log.iter().map(|(_, b)| *b).sum()
    }

    /// Downloads > "When all downloads finish": fire once everything that was
    /// running or waiting is done.
    fn check_all_done(self: &Arc<Self>) {
        let action = {
            let mut inner = self.inner.lock().unwrap();
            if inner.when_all_done == AfterDone::Nothing {
                return;
            }
            let busy = inner
                .entries
                .values()
                .any(|e| e.task.is_some() || e.rec.status == Status::Queued);
            if busy {
                inner.all_done_seen_active = true;
                return;
            }
            if !inner.all_done_seen_active {
                return;
            }
            inner.all_done_seen_active = false;
            std::mem::take(&mut inner.when_all_done)
        };
        self.after_done(action);
    }

    /// Every queue's own schedule (start / stop times, days, once on a date),
    /// and its "when done" action once all its files are finished.
    fn check_schedule(self: &Arc<Self>) {
        let queues = self.settings.read().unwrap().queues.clone();
        let now = chrono::Local::now();
        let key = (now.ordinal(), now.hour(), now.minute());
        let day = now.weekday().num_days_from_monday() as usize;
        let today = now.format("%Y-%m-%d").to_string();
        let new_minute = {
            let mut inner = self.inner.lock().unwrap();
            if inner.sched_minute == Some(key) {
                false
            } else {
                inner.sched_minute = Some(key);
                true
            }
        };
        if new_minute {
            let hm = Some((now.hour(), now.minute()));
            for q in &queues {
                let sc = &q.schedule;
                let day_ok = if sc.once { sc.once_date.trim() == today } else { sc.days[day] };
                if sc.start_enabled && day_ok && parse_hhmm(&sc.start_time) == hm {
                    self.start_queue(q.id);
                }
                if sc.stop_enabled && parse_hhmm(&sc.stop_time) == hm {
                    self.stop_queue(q.id);
                }
            }
        }
        // queues that finished all their files
        let mut done = Vec::new();
        {
            let mut inner = self.inner.lock().unwrap();
            let armed: Vec<u64> = inner.queue_armed.iter().copied().collect();
            for q in armed {
                let busy = inner
                    .entries
                    .values()
                    .any(|e| e.rec.queue == Some(q) && (e.task.is_some() || e.rec.status == Status::Queued));
                if !busy {
                    inner.queue_armed.remove(&q);
                    inner.events.push(Event::QueueDone(q));
                    done.push(q);
                }
            }
        }
        for q in done {
            if let Some(def) = queues.iter().find(|d| d.id == q) {
                self.after_done(def.after_done);
            }
        }
    }

    /// Exit at once; shut down / sleep / hibernate go through the app's
    /// countdown window (it calls `system::power` when the time is up).
    fn after_done(&self, action: AfterDone) {
        match action {
            AfterDone::Nothing => {}
            AfterDone::Exit => self.inner.lock().unwrap().exit_requested = true,
            AfterDone::Sleep | AfterDone::Shutdown | AfterDone::Hibernate => {
                self.inner.lock().unwrap().power_request = Some(action);
            }
        }
    }
}

/// The name an MP3 file shows: its file name without the quality part.
fn song_title(file_name: &str) -> String {
    let (stem, _) = filename::split_ext(file_name);
    let t = stem.trim();
    match t.rfind(" [") {
        Some(i) if t.ends_with(']') => t[..i].trim().to_string(),
        _ => t.to_string(),
    }
}

/// Bytes from the start of a file with no gap.
fn ready_from_start(segs: &[segments::Segment]) -> u64 {
    let mut ready = 0u64;
    let mut sorted: Vec<_> = segs.iter().map(|s| (s.start, s.pos)).collect();
    sorted.sort();
    for (start, pos) in sorted {
        if start > ready {
            break;
        }
        ready = ready.max(pos);
    }
    ready
}

/// What a running download offers to play (see `Manager::preview_source`).
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewSource {
    /// one file that grows
    File { path: PathBuf, ready: u64 },
    /// a picture file and a sound file (no sound yet: `audio` is `None`)
    Pair { video: PathBuf, video_ready: u64, audio: Option<PathBuf>, audio_ready: u64 },
}

/// `path` is in `dir` (or below it), with no ".." that could lead out.
fn inside(path: &Path, dir: &Path) -> bool {
    path.starts_with(dir)
        && path != dir
        && !path.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::CurDir))
}

/// The files of a torrent, then its folders if they are empty now.
fn remove_torrent_data(rec: &DownloadRecord) -> bool {
    let mut ok = true;
    // only inside its own folder: a strange name can't reach anything else
    let files: Vec<PathBuf> = rec.torrent_data_paths().into_iter().filter(|f| inside(f, &rec.save_dir)).collect();
    for f in &files {
        if f.is_file() {
            ok &= std::fs::remove_file(f).is_ok();
        }
    }
    let top = rec.target_path();
    if !inside(&top, &rec.save_dir) {
        return ok;
    }
    let mut dirs: Vec<PathBuf> = files.iter().filter_map(|f| f.parent().map(Path::to_path_buf)).filter(|d| d.starts_with(&top)).collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    dirs.dedup();
    for d in dirs.iter().chain(std::iter::once(&top)) {
        let _ = std::fs::remove_dir(d); // only when empty
    }
    ok
}

/// Delete every temporary file of a download. Returns false if something
/// could not be removed (still open), so the caller can retry.
fn remove_temp_files(rec: &DownloadRecord) -> bool {
    let mut ok = true;
    if rec.kind == DownloadKind::Torrent && rec.status != Status::Completed {
        // what an unfinished torrent got so far is its partial data
        ok &= remove_torrent_data(rec);
    }
    for p in rec.temp_paths() {
        if p.is_dir() {
            ok &= std::fs::remove_dir_all(&p).is_ok();
        } else if p.exists() {
            ok &= std::fs::remove_file(&p).is_ok();
        }
    }
    ok
}

fn make_view(e: &Entry) -> DownloadView {
    view_of(e, true)
}

fn view_of(e: &Entry, with_parts: bool) -> DownloadView {
    let bytes = e.progress.bytes.load(Ordering::Relaxed);
    let mut status = e.rec.status;
    if status == Status::Downloading && e.progress.assembling.load(Ordering::Relaxed) {
        status = Status::Assembling;
    }
    let downloaded = if status == Status::Completed {
        e.rec.total_size.unwrap_or(bytes.max(e.rec.downloaded))
    } else {
        bytes
    };
    let speed = if status.is_running() { e.speed.value } else { 0.0 };
    let eta_secs = match e.rec.total_size {
        Some(t) if speed > 1.0 => Some((t.saturating_sub(downloaded) as f64 / speed) as u64),
        _ => None,
    };
    let segments = if status == Status::Completed || !with_parts {
        Vec::new()
    } else {
        e.segs.lock().unwrap().iter().map(|s| (s.start, s.pos, s.end, s.active)).collect()
    };
    DownloadView {
        id: e.rec.id,
        url: e.rec.url.clone(),
        file_name: e.rec.file_name.clone(),
        save_path: e.rec.target_path(),
        category: e.rec.category.clone(),
        kind: e.rec.kind,
        status,
        error: e.rec.error.clone(),
        total_size: e.rec.total_size,
        downloaded,
        speed,
        eta_secs,
        active_connections: e.progress.connections.load(Ordering::Relaxed),
        waiting: status.is_running()
            && e.progress.waiting.load(Ordering::Relaxed)
            && e.progress.connections.load(Ordering::Relaxed) == 0,
        resumable: e.rec.resumable,
        parts_total: e.progress.parts_total.load(Ordering::Relaxed),
        parts_done: e.progress.parts_done.load(Ordering::Relaxed),
        segments,
        queue: e.rec.queue,
        finished_at: e.rec.finished_at,
        description: e.rec.description.clone(),
        speed_limit_kb: e.rec.speed_limit_kb,
        on_complete: e.rec.on_complete,
        added_at: e.rec.added_at,
        last_try: e.rec.last_try,
        headers: e.rec.headers.clone(),
        parent_page: e.rec.source_page.clone().or_else(|| e.rec.headers.referer.clone()),
        queue_pos: e.rec.queue_pos,
    }
}

/// Editable fields of the File Properties window.
#[derive(Debug, Clone, Default)]
pub struct DetailsEdit {
    pub url: Option<String>,
    pub referer: Option<String>,
    pub description: Option<String>,
    pub auth: Option<(String, String)>,
}

/// The download's headers plus a saved site login, if it has none of its own.
fn with_login(s: &Settings, url: &str, h: &RequestHeaders) -> RequestHeaders {
    let mut h = h.clone();
    if h.auth.is_none() {
        h.auth = s.login_for(url);
    }
    h
}

fn collect_records(inner: &Inner) -> Vec<DownloadRecord> {
    inner
        .entries
        .values()
        .map(|e| {
            let mut r = e.rec.clone();
            if r.status != Status::Completed {
                r.segments = e.segs.lock().unwrap().clone();
                r.audio_segments = e.audio_segs.lock().unwrap().clone();
                r.downloaded = e.progress.bytes.load(Ordering::Relaxed);
                r.parts_total = e.progress.parts_total.load(Ordering::Relaxed);
                r.parts_done = e.progress.parts_done.load(Ordering::Relaxed);
            }
            r
        })
        .collect()
}

/// The file name of a playlist video: its own `title`, after the "07 - "
/// number the playlist gave it (when it has one).
fn playlist_stem(current: &str, title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return current.to_string();
    }
    let digits = current.chars().take_while(|c| c.is_ascii_digit()).count();
    let number = if digits > 0 && current[digits..].starts_with(" - ") { &current[..digits + 3] } else { "" };
    format!("{number}{title}")
}

/// A name that is free on disk and not used by another unfinished download.
fn free_name(inner: &Inner, dir: &Path, wanted: &str) -> String {
    let used = |n: &str| {
        inner
            .entries
            .values()
            .any(|e| e.rec.status != Status::Completed && e.rec.save_dir == dir && e.rec.file_name == n)
    };
    let first = filename::unique_name(dir, wanted);
    if !used(&first) {
        return first;
    }
    let (stem, ext) = filename::split_ext(wanted);
    for i in 1..10_000 {
        let candidate = if ext.is_empty() {
            format!("{stem} ({i})")
        } else {
            format!("{stem} ({i}).{ext}")
        };
        if !used(&candidate) && filename::unique_name(dir, &candidate) == candidate {
            return candidate;
        }
    }
    first
}

#[cfg(test)]
mod name_tests {
    use super::playlist_stem;

    #[test]
    fn playlist_videos_get_their_own_title() {
        // the playlist page showed a translated title; the video's own one wins
        assert_eq!(playlist_stem("07 - How to cook rice", "طريقة عمل الأرز"), "07 - طريقة عمل الأرز");
        assert_eq!(playlist_stem("How to cook rice", "طريقة عمل الأرز"), "طريقة عمل الأرز");
        assert_eq!(playlist_stem("2024 plans", "Plans"), "Plans");
        assert_eq!(playlist_stem("07 - Old", "  "), "07 - Old");
    }
}

#[cfg(test)]
mod inside_tests {
    use super::{inside, song_title};
    #[test]
    fn mp3_titles() {
        assert_eq!(song_title("My Song [mp3].mp3"), "My Song");
        assert_eq!(song_title("Live [2024] mix.mp3"), "Live [2024] mix");
        assert_eq!(song_title("plain.mp3"), "plain");
    }
    use std::path::Path;
    #[test]
    fn nothing_outside_the_folder() {
        let d = Path::new("/d/Video");
        assert!(inside(Path::new("/d/Video/pack/a.bin"), d));
        assert!(!inside(Path::new("/d/Video/../x.bin"), d));
        assert!(!inside(Path::new("/d/Video"), d));
        assert!(!inside(Path::new("/d/Other/a.bin"), d));
    }
}
