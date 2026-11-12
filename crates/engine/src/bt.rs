//! Torrents, shared by librqbit inside the program (no helper program).
//!
//! One session for the whole program, started when the first torrent needs
//! it. It remembers which pieces of each torrent are on the disk (a stopped
//! torrent goes on without checking every piece again, after a restart too)
//! and the people it found (the DHT list), in the program's own folder.
//!
//! Files are written by our own store ([`SafeFiles`]): every part of a name
//! made safe for Windows first, the same way the .torrent window shows it.

use crate::sync::LockOk;
use crate::torrent::{
    is_magnet, is_tracker, parse_torrent, safe_part, AddOptions, FileState, Peer, TorrentStatus, PUBLIC_TRACKERS,
};
use anyhow::{anyhow, bail, Context};
use librqbit::api::TorrentIdOrHash;
use librqbit::limits::LimitsConfig;
use librqbit::storage::filesystem::FilesystemStorageFactory;
use librqbit::storage::{StorageFactory, StorageFactoryExt, TorrentStorage};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerMode, ListenerOptions, ManagedTorrent,
    ManagedTorrentShared, Session, SessionOptions, SessionPersistenceConfig, TorrentMetadata, TorrentStatsState,
};
use librqbit_core::hash_id::Id20;
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Handle = Arc<ManagedTorrent>;

/// Where each file of a torrent goes, under the folder it is saved in: a
/// torrent of many files in a folder of its own name, every part of every
/// name made safe ("CON", "a:b", "x." can't be kept by Windows as they are).
pub fn disk_paths(metadata: &TorrentMetadata) -> Vec<Option<PathBuf>> {
    let info = metadata.info.info();
    let many = info.files.is_some();
    let top = metadata.info.name().map(|n| safe_part(&n)).unwrap_or_else(|| "torrent".into());
    metadata
        .file_infos
        .iter()
        .map(|f| {
            if f.attrs.padding {
                return None;
            }
            let mut p = PathBuf::new();
            if many {
                p.push(&top);
            }
            for part in f.relative_filename.iter() {
                p.push(safe_part(&part.to_string_lossy()));
            }
            Some(p)
        })
        .collect()
}

/// Each torrent's folder (by its id): librqbit keeps it to itself.
type Folders = Arc<Mutex<HashMap<String, PathBuf>>>;

/// Our file store for librqbit: files at their safe names.
#[derive(Clone, Default)]
pub struct SafeFiles {
    folders: Folders,
}

impl StorageFactory for SafeFiles {
    type Storage = SafeStore;

    fn create(&self, shared: &ManagedTorrentShared, _metadata: &TorrentMetadata) -> anyhow::Result<SafeStore> {
        let root = self.folders.lock_ok().get(&shared.info_hash.as_string()).cloned().context("the torrent's folder is not known")?;
        Ok(SafeStore { root, files: Arc::new(Vec::new()) })
    }

    // librqbit keeps its notes of what is on the disk (to go on quickly) only
    // for plain files on the disk; these are such files, at safer names
    fn is_type_id(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<Self>() || type_id == TypeId::of::<FilesystemStorageFactory>()
    }

    fn clone_box(&self) -> librqbit::storage::BoxStorageFactory {
        self.clone().boxed()
    }
}

/// One file of a torrent: where it goes, opened when first needed (the
/// files that were not chosen are mostly never made).
struct Slot {
    path: PathBuf,
    file: std::sync::OnceLock<File>,
}

/// The files of one torrent (none for the padding between files).
pub struct SafeStore {
    root: PathBuf,
    files: Arc<Vec<Option<Slot>>>,
}

impl SafeStore {
    fn slot(&self, id: usize) -> anyhow::Result<&Slot> {
        match self.files.get(id) {
            Some(Some(s)) => Ok(s),
            Some(None) => bail!("padding has no file"),
            None => bail!("no such file in the torrent"),
        }
    }

    /// The open file; `make`: made when it is not there yet.
    fn open(&self, id: usize, make: bool) -> anyhow::Result<&File> {
        let s = self.slot(id)?;
        if let Some(f) = s.file.get() {
            return Ok(f);
        }
        if !make && !s.path.exists() {
            bail!("{} is not there yet", s.path.display());
        }
        if let Some(dir) = s.path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("the folder {} can't be made", dir.display()))?;
        }
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&s.path)
            .with_context(|| format!("{} can't be opened", s.path.display()))?;
        // a piece written far into the file: no waiting for Windows to fill
        // everything before it with zeros
        crate::fileio::make_sparse(&f);
        let _ = s.file.set(f);
        s.file.get().context("the file was not kept")
    }
}

impl TorrentStorage for SafeStore {
    fn init(&mut self, _shared: &ManagedTorrentShared, metadata: &TorrentMetadata) -> anyhow::Result<()> {
        let root = self.root.clone();
        self.files = Arc::new(
            disk_paths(metadata)
                .into_iter()
                .map(|p| p.map(|p| Slot { path: root.join(p), file: Default::default() }))
                .collect(),
        );
        Ok(())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        Ok(crate::fileio::read_at(self.open(file_id, false)?, buf, offset)?)
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        Ok(crate::fileio::write_at(self.open(file_id, true)?, buf, offset)?)
    }

    fn remove_file(&self, file_id: usize, _filename: &Path) -> anyhow::Result<()> {
        match self.files.get(file_id) {
            Some(Some(s)) if s.path.exists() => Ok(std::fs::remove_file(&s.path)?),
            _ => Ok(()),
        }
    }

    fn remove_directory_if_empty(&self, path: &Path) -> anyhow::Result<()> {
        let dir = self.root.join(path);
        if dir.is_dir() && std::fs::read_dir(&dir)?.next().is_none() {
            std::fs::remove_dir(&dir)?;
        }
        Ok(())
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        if matches!(self.files.get(file_id), Some(None)) {
            return Ok(());
        }
        let f = self.open(file_id, true)?;
        if f.metadata()?.len() != length {
            f.set_len(length)?;
        }
        Ok(())
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(SafeStore { root: self.root.clone(), files: self.files.clone() }))
    }
}

/// What a running torrent can be changed to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Live {
    /// files to get, 1-based ("1,3-5"); None = all
    pub select: Option<String>,
    /// trackers the user added
    pub trackers: Vec<String>,
    /// the first and last pieces of the biggest file first
    pub first_last: bool,
}

/// "1,3-5" (1-based, as the window gives it) -> 0-based file numbers.
pub fn parse_select(sel: &str, count: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for part in sel.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>().ok(), b.trim().parse::<usize>().ok()),
            None => {
                let n = part.parse::<usize>().ok();
                (n, n)
            }
        };
        if let (Some(a), Some(b)) = (a, b) {
            for i in a.max(1)..=b.min(count) {
                if !out.contains(&(i - 1)) {
                    out.push(i - 1);
                }
            }
        }
    }
    out.sort_unstable();
    out
}

/// The chosen files of a torrent given as its bytes.
fn h_only(select: &Option<String>, add: &AddTorrent<'_>) -> Option<Vec<usize>> {
    let AddTorrent::TorrentFileBytes(b) = add else { return None };
    let n = parse_torrent(b).ok()?.files.len();
    select.as_deref().filter(|s| !s.trim().is_empty()).map(|s| parse_select(s, n)).filter(|v| !v.is_empty())
}

/// The torrent's id (its info hash, 40 hex) from a magnet link or a .torrent file.
pub fn hash_of(source: &str) -> Option<String> {
    if is_magnet(source) {
        let i = source.to_ascii_lowercase().find("xt=urn:btih:")? + "xt=urn:btih:".len();
        let h: String = source[i..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        // 40 hex, or 32 base32 (either case)
        return Id20::from_str(&h).or_else(|_| Id20::from_str(&h.to_ascii_uppercase())).ok().map(|id| id.as_string());
    }
    let data = std::fs::read(source).ok()?;
    parse_torrent(&data).ok()?.info_hash
}

#[derive(Default)]
struct PeerSeen {
    at: Option<Instant>,
    /// address -> bytes from them, bytes to them
    bytes: HashMap<String, (u64, u64)>,
    /// how many sent something since the last look
    sending: u32,
}

/// The torrents of the program.
pub struct Torrents {
    data_dir: PathBuf,
    session: tokio::sync::OnceCell<Arc<Session>>,
    seen: Mutex<HashMap<String, PeerSeen>>,
    /// "first and last pieces first": the open streams that ask for them
    heads: Mutex<HashMap<String, Vec<Box<dyn std::any::Any + Send>>>>,
    folders: Folders,
    /// a download's own speed limit (KB/s) each torrent was started with
    own_limit: Mutex<HashMap<String, u64>>,
    /// torrents that share for a while after they are complete (a number,
    /// so a later start of the same torrent ends the old sharing)
    sharing: Mutex<HashMap<String, u64>>,
    turns: std::sync::atomic::AtomicU64,
}

impl Torrents {
    pub fn new(data_dir: &Path) -> Torrents {
        Torrents {
            data_dir: data_dir.to_path_buf(),
            session: tokio::sync::OnceCell::new(),
            seen: Mutex::default(),
            heads: Mutex::default(),
            folders: Folders::default(),
            own_limit: Mutex::default(),
            sharing: Mutex::default(),
            turns: Default::default(),
        }
    }

    async fn session(&self) -> anyhow::Result<&Arc<Session>> {
        self.session
            .get_or_try_init(|| async {
                std::fs::create_dir_all(&self.data_dir)?;
                // the folders of the torrents it kept from last time
                let saved = self.data_dir.join("session").join("session.json");
                if let Ok(v) = std::fs::read(&saved).map_err(anyhow::Error::from).and_then(|b| Ok(serde_json::from_slice::<serde_json::Value>(&b)?)) {
                    if let Some(all) = v["torrents"].as_object() {
                        let mut f = self.folders.lock_ok();
                        for t in all.values() {
                            if let (Some(h), Some(dir)) = (t["info_hash"].as_str(), t["output_folder"].as_str()) {
                                f.insert(h.to_ascii_lowercase(), PathBuf::from(dir));
                            }
                        }
                    }
                }
                // no IPv6 on this computer (turned off, or none): IPv4 only
                let v4 = std::net::TcpListener::bind((std::net::Ipv6Addr::UNSPECIFIED, 0)).is_err();
                let trackers: HashSet<url::Url> = PUBLIC_TRACKERS.iter().filter_map(|t| url::Url::parse(t).ok()).collect();
                let opts = SessionOptions {
                    dht: Some(DhtSessionConfig {
                        persistence: Some(librqbit::dht::DhtPersistenceConfig {
                            config_filename: Some(self.data_dir.join("dht.json")),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    fastresume: true,
                    persistence: Some(SessionPersistenceConfig::Json { folder: Some(self.data_dir.join("session")) }),
                    listen: Some(ListenerOptions {
                        mode: ListenerMode::TcpOnly,
                        enable_upnp_port_forwarding: true,
                        listen_addr: if v4 { (std::net::Ipv4Addr::UNSPECIFIED, 0).into() } else { (std::net::Ipv6Addr::UNSPECIFIED, 0).into() },
                        ipv4_only: v4,
                        ..Default::default()
                    }),
                    ipv4_only: v4,
                    default_storage_factory: Some(SafeFiles { folders: self.folders.clone() }.boxed()),
                    trackers,
                    peer_limit: Some(150),
                    // the streams of "first and last pieces first" each keep one
                    runtime_worker_threads: Some(16),
                    ..Default::default()
                };
                let session = Session::new_with_opts(self.data_dir.join("unused"), opts).await.context("torrents can't start")?;
                // what ran last time waits until its download is started again
                let running: Vec<Handle> =
                    session.with_torrents(|all| all.filter(|(_, t)| !t.is_paused()).map(|(_, t)| t.clone()).collect());
                for t in running {
                    let _ = session.pause(&t).await;
                }
                Ok::<_, anyhow::Error>(session)
            })
            .await
    }

    fn id(gid: &str) -> anyhow::Result<TorrentIdOrHash> {
        Ok(TorrentIdOrHash::Hash(Id20::from_str(gid).map_err(|_| anyhow!("not a torrent id"))?))
    }

    async fn handle(&self, gid: &str) -> anyhow::Result<Handle> {
        self.session().await?.get(Self::id(gid)?).ok_or_else(|| anyhow!("the torrent was stopped"))
    }

    /// The speed limits of all torrents together (bytes/s, 0 = none).
    pub async fn set_limits(&self, down: u64, up: u64) {
        let Some(session) = self.session.get() else { return };
        let bps = |b: u64| NonZeroU32::new(b.min(u32::MAX as u64) as u32);
        if session.ratelimits.get_download_bps() != bps(down) {
            session.ratelimits.set_download_bps(bps(down));
        }
        if session.ratelimits.get_upload_bps() != bps(up) {
            session.ratelimits.set_upload_bps(bps(up));
        }
    }

    /// Start (or go on with) a torrent; gives its id.
    pub async fn add(&self, source: &str, o: &AddOptions) -> anyhow::Result<String> {
        let session = self.session().await?.clone();
        let source = source.trim();
        let mut count = None;
        let add = if is_magnet(source) {
            AddTorrent::from_url(source.to_string())
        } else {
            let data = std::fs::read(source).context("the .torrent file can't be read")?;
            count = Some(parse_torrent(&data)?.files.len());
            AddTorrent::from_bytes(data)
        };
        // the chosen files from the start (known for a .torrent file)
        let only = count.zip(o.select.as_deref().filter(|s| !s.trim().is_empty())).map(|(n, s)| parse_select(s, n)).filter(|v| !v.is_empty());
        // known already: as it was asked now
        if let Some(gid) = hash_of(source) {
            self.sharing.lock_ok().remove(&gid);
            if let Some(h) = session.get(Self::id(&gid)?) {
                let same = h.output_folder() == o.dir
                    && o.trackers.iter().all(|t| !is_tracker(t) || h.shared().trackers.iter().any(|u| u.as_str() == t.trim()))
                    && self.own_limit.lock_ok().get(&gid).copied().unwrap_or(0) == o.download_limit_kb;
                if o.check || self.files_gone(&h) {
                    // checked again from nothing
                    self.forget(&gid).await;
                } else if !same {
                    let bytes = h.with_metadata(|m| m.torrent_bytes.clone())?;
                    let h = self.start_again(&gid, AddTorrent::from_bytes(bytes), o).await?;
                    let _ = self.apply(&gid, &h, o.select.as_deref(), o.first_last).await;
                    return Ok(gid);
                } else {
                    let _ = self.apply(&gid, &h, o.select.as_deref(), o.first_last).await;
                    if h.is_paused() {
                        session.unpause(&h).await?;
                    }
                    return Ok(gid);
                }
            }
            self.folders.lock_ok().insert(gid.clone(), o.dir.clone());
        }
        let h = self.start(add, o, only).await?;
        let gid = h.info_hash().as_string();
        self.sharing.lock_ok().remove(&gid);
        // (a magnet link's files are chosen once it is ready: the next change)
        let _ = self.apply(&gid, &h, o.select.as_deref(), o.first_last).await;
        Ok(gid)
    }

    async fn start(&self, add: AddTorrent<'_>, o: &AddOptions, only: Option<Vec<usize>>) -> anyhow::Result<Handle> {
        let session = self.session().await?.clone();
        let limit = |kb: u64| NonZeroU32::new(kb.saturating_mul(1024).min(u32::MAX as u64) as u32);
        let opts = AddTorrentOptions {
            overwrite: true,
            output_folder: Some(o.dir.to_string_lossy().into_owned()),
            ratelimits: LimitsConfig { download_bps: limit(o.download_limit_kb), upload_bps: None },
            trackers: Some(o.trackers.iter().map(|t| t.trim().to_string()).filter(|t| is_tracker(t)).collect()),
            only_files: only,
            ..Default::default()
        };
        let h = match session.add_torrent(add, Some(opts)).await? {
            AddTorrentResponse::Added(_, h) | AddTorrentResponse::AlreadyManaged(_, h) => h,
            AddTorrentResponse::ListOnly(_) => bail!("the torrent was not started"),
        };
        if h.is_paused() {
            session.unpause(&h).await?;
        }
        let gid = h.info_hash().as_string();
        self.folders.lock_ok().insert(gid.clone(), o.dir.clone());
        self.own_limit.lock_ok().insert(gid, o.download_limit_kb);
        Ok(h)
    }

    /// Started again with other options (folder, trackers, its own limit):
    /// librqbit forgets a torrent with its notes of which pieces are here,
    /// so the notes are kept aside meanwhile and nothing is checked again.
    async fn start_again(&self, gid: &str, add: AddTorrent<'_>, o: &AddOptions) -> anyhow::Result<Handle> {
        let session = self.session().await?.clone();
        let notes_dir = self.data_dir.join("session");
        let notes = std::fs::read_dir(&notes_dir)
            .ok()
            .and_then(|d| {
                d.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
                    p.extension().is_some_and(|x| x == "bitv")
                        && p.file_stem().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(gid))
                })
            });
        let kept = notes.as_ref().and_then(|n| std::fs::read(n).ok());
        if let Some(h) = session.get(Self::id(gid)?) {
            let _ = session.pause(&h).await;
        }
        self.heads.lock_ok().remove(gid);
        session.delete(Self::id(gid)?, false).await.ok();
        if let (Some(n), Some(bytes)) = (&notes, &kept) {
            let _ = std::fs::write(n, bytes);
        }
        self.folders.lock_ok().insert(gid.to_string(), o.dir.clone());
        let only = h_only(&o.select, &add);
        self.start(add, o, only).await
    }

    /// Files it had on the disk are gone (deleted while it was stopped):
    /// checked again from nothing, not taken as there.
    fn files_gone(&self, h: &Handle) -> bool {
        let only = h.only_files();
        let st = h.stats();
        h.with_metadata(|m| {
            disk_paths(m).iter().enumerate().any(|(i, p)| {
                let wanted = only.as_ref().is_none_or(|o| o.contains(&i));
                let had = st.file_progress.get(i).copied().unwrap_or(0) > 0;
                p.as_ref().is_some_and(|p| wanted && had && !h.output_folder().join(p).exists())
            })
        })
        .unwrap_or(false)
    }

    /// The files to get, and "first and last pieces first".
    async fn apply(&self, gid: &str, h: &Handle, select: Option<&str>, first_last: bool) -> anyhow::Result<()> {
        if matches!(h.stats().state, TorrentStatsState::Initializing { .. }) {
            bail!("the torrent is not ready yet");
        }
        let count = h.with_metadata(|m| m.file_infos.len()).unwrap_or(0);
        if count > 0 {
            let want: HashSet<usize> = match select.filter(|s| !s.trim().is_empty()) {
                Some(s) => parse_select(s, count).into_iter().collect(),
                None => (0..count).collect(),
            };
            let now: HashSet<usize> = h.only_files().map(|v| v.into_iter().collect()).unwrap_or_else(|| (0..count).collect());
            if !want.is_empty() && want != now {
                self.session().await?.update_only_files(h, &want).await?;
            }
        }
        self.first_last(gid, h, first_last).await;
        Ok(())
    }

    /// Two streams on the biggest wanted file, one at its start and one at
    /// its end: librqbit asks for the pieces just ahead of each first.
    async fn first_last(&self, gid: &str, h: &Handle, on: bool) {
        self.heads.lock_ok().remove(gid);
        if !on || h.stats().finished {
            return;
        }
        let only = h.only_files();
        let biggest = h
            .with_metadata(|m| {
                m.file_infos
                    .iter()
                    .enumerate()
                    .filter(|(i, f)| !f.attrs.padding && only.as_ref().is_none_or(|o| o.contains(i)))
                    .max_by_key(|(_, f)| f.len)
                    .map(|(i, f)| (i, f.len))
            })
            .ok()
            .flatten();
        let Some((file, len)) = biggest else { return };
        let mut streams = Vec::new();
        for back in [None, Some(len.saturating_sub(2 << 20))] {
            let Ok(Ok(mut s)) = tokio::time::timeout(Duration::from_secs(5), h.clone().stream(file)).await else { break };
            if let Some(pos) = back {
                use tokio::io::AsyncSeekExt;
                if s.seek(std::io::SeekFrom::Start(pos)).await.is_err() {
                    continue;
                }
            }
            streams.push(Box::new(s) as Box<dyn std::any::Any + Send>);
        }
        if !streams.is_empty() {
            self.heads.lock_ok().insert(gid.to_string(), streams);
        }
    }

    pub async fn status(&self, gid: &str) -> anyhow::Result<TorrentStatus> {
        let h = self.handle(gid).await?;
        let s = h.stats();
        let status = match s.state {
            TorrentStatsState::Initializing { .. } => "waiting",
            TorrentStatsState::Live => "active",
            TorrentStatsState::Paused => "paused",
            TorrentStatsState::Error => "error",
        };
        let (speed, upload_speed, connections) = s
            .live
            .as_ref()
            .map(|l| (l.download_speed.as_bytes(), l.upload_speed.as_bytes(), l.snapshot.peer_stats.live))
            .unwrap_or_default();
        let (pieces, bitfield) = self.haves(&h);
        let mut trackers: Vec<String> = h.shared().trackers.iter().map(|u| u.to_string()).collect();
        for t in PUBLIC_TRACKERS {
            if !trackers.iter().any(|x| x == t) {
                trackers.push((*t).to_string());
            }
        }
        Ok(TorrentStatus {
            status: status.to_string(),
            total: s.total_bytes,
            done: s.progress_bytes,
            speed,
            upload_speed,
            connections,
            seeders: self.seen.lock_ok().get(gid).map(|p| p.sending).unwrap_or(0),
            seeding: s.finished,
            name: h.name().filter(|n| !n.is_empty()),
            followed_by: None,
            error: s.error.clone(),
            pieces,
            bitfield,
            trackers,
            upload_total: s.uploaded_bytes,
        })
    }

    /// How many pieces, and which are here (hex, a bit a piece).
    fn haves(&self, h: &Handle) -> (u32, String) {
        let Some(session) = self.session.get() else { return (0, String::new()) };
        let api = librqbit::Api::new(session.clone(), None);
        match api.api_dump_haves(TorrentIdOrHash::Hash(h.info_hash())) {
            Ok((bf, n)) => (n, bf.as_raw_slice().iter().map(|b| format!("{b:02x}")).collect()),
            Err(_) => (0, String::new()),
        }
    }

    /// The people this torrent is connected to (speeds: since the last look).
    pub async fn peers(&self, gid: &str) -> anyhow::Result<Vec<Peer>> {
        let h = self.handle(gid).await?;
        let Some(live) = h.live() else { return Ok(Vec::new()) };
        let snap = live.per_peer_stats_snapshot(Default::default());
        let mut seen = self.seen.lock_ok();
        let p = seen.entry(gid.to_string()).or_default();
        let secs = p.at.map(|a| a.elapsed().as_secs_f64()).filter(|s| *s > 0.2);
        let mut out = Vec::new();
        let mut now = HashMap::new();
        let mut sending = 0;
        for (addr, st) in snap.peers.iter().take(300) {
            let (got, gave) = (st.counters.fetched_bytes, st.counters.uploaded_bytes);
            let (got0, gave0) = p.bytes.get(addr).copied().unwrap_or((got, gave));
            let per = |a: u64, b: u64| secs.map(|s| (a.saturating_sub(b) as f64 / s) as u64).unwrap_or(0);
            let down = per(got, got0);
            if down > 0 {
                sending += 1;
            }
            out.push(Peer { address: addr.clone(), down, up: per(gave, gave0), have: -1.0, seeder: false });
            now.insert(addr.clone(), (got, gave));
        }
        p.bytes = now;
        p.at = Some(Instant::now());
        p.sending = sending;
        out.sort_by(|a, b| b.down.cmp(&a.down).then(b.up.cmp(&a.up)));
        Ok(out)
    }

    /// The files of a torrent once they are known, with how much of each is here.
    pub async fn files(&self, gid: &str) -> anyhow::Result<Vec<FileState>> {
        let h = self.handle(gid).await?;
        let s = h.stats();
        let only = h.only_files();
        let root = h.output_folder().to_path_buf();
        Ok(h.with_metadata(|m| {
            disk_paths(m)
                .into_iter()
                .zip(m.file_infos.iter())
                .enumerate()
                .filter_map(|(i, (p, f))| {
                    Some(FileState {
                        index: i + 1,
                        path: root.join(p?),
                        size: f.len,
                        done: s.file_progress.get(i).copied().unwrap_or(0).min(f.len),
                        selected: only.as_ref().is_none_or(|o| o.contains(&i)),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
    }

    /// Change what a running torrent gets. New trackers: the torrent starts
    /// again with them (nothing is checked again, nothing is lost).
    pub async fn change(&self, gid: &str, live: &Live) -> anyhow::Result<()> {
        let mut h = self.handle(gid).await?;
        let missing = live.trackers.iter().any(|t| is_tracker(t) && !h.shared().trackers.iter().any(|u| u.as_str() == t.trim()));
        if missing {
            let bytes = h.with_metadata(|m| m.torrent_bytes.clone())?;
            let o = AddOptions {
                dir: h.output_folder().to_path_buf(),
                download_limit_kb: self.own_limit.lock_ok().get(gid).copied().unwrap_or(0),
                trackers: live.trackers.clone(),
                ..Default::default()
            };
            h = self.start_again(gid, AddTorrent::from_bytes(bytes), &o).await?;
            // what is on the disk is taken from the notes: ready in a moment
            let _ = tokio::time::timeout(Duration::from_secs(30), h.wait_until_initialized()).await;
        }
        self.apply(gid, &h, live.select.as_deref(), live.first_last).await
    }

    /// The file list of a magnet link before it is downloaded: only the
    /// torrent's own description is fetched from the people who share it,
    /// into `dir` (a .torrent file, given back). Gives up after `wait`.
    pub async fn metadata(&self, magnet: &str, dir: &Path, wait: Duration) -> anyhow::Result<PathBuf> {
        if !is_magnet(magnet) {
            bail!("not a magnet link");
        }
        std::fs::create_dir_all(dir)?;
        let session = self.session().await?.clone();
        let opts = AddTorrentOptions { list_only: true, ..Default::default() };
        let r = tokio::time::timeout(wait, session.add_torrent(AddTorrent::from_url(magnet.trim().to_string()), Some(opts)))
            .await
            .map_err(|_| anyhow!("nobody sent the file list in time"))??;
        let AddTorrentResponse::ListOnly(r) = r else { bail!("the file list could not be fetched") };
        let path = dir.join(format!("{}.torrent", r.info_hash.as_string()));
        std::fs::write(&path, &r.torrent_bytes)?;
        parse_torrent(&r.torrent_bytes).context("the file list that came is not a torrent")?;
        Ok(path)
    }

    /// People are connected but nobody sends anything: drop them and ask the
    /// trackers and the DHT again (a stop and a start do both), so other
    /// people, who have the pieces, get a turn.
    pub async fn look_for_others(&self, gid: &str) -> anyhow::Result<()> {
        let h = self.handle(gid).await?;
        let session = self.session().await?.clone();
        self.heads.lock_ok().remove(gid);
        session.pause(&h).await?;
        session.unpause(&h).await?;
        Ok(())
    }

    /// Stop it now: what is here stays, and the notes of which pieces are
    /// here, so it goes on from there next time.
    pub async fn remove(&self, gid: &str) {
        self.heads.lock_ok().remove(gid);
        self.seen.lock_ok().remove(gid);
        if let (Some(session), Ok(id)) = (self.session.get(), Self::id(gid)) {
            if let Some(h) = session.get(id) {
                if !h.is_paused() {
                    let _ = session.pause(&h).await;
                }
            }
        }
    }

    /// Forget it altogether (its files stay where they are).
    pub async fn forget(&self, gid: &str) {
        self.heads.lock_ok().remove(gid);
        self.seen.lock_ok().remove(gid);
        self.sharing.lock_ok().remove(gid);
        if let (Some(session), Ok(id)) = (self.session.get(), Self::id(gid)) {
            let _ = session.delete(id, false).await;
        }
    }

    /// Complete: it shares with others until `minutes` passed or it sent
    /// `ratio` times its size, whichever comes first (0 = that one is no
    /// limit; both 0: no sharing), then it is forgotten and `leftovers`
    /// (bits of files that were not chosen) are deleted, with their folder
    /// when it is left empty.
    pub fn share_then_forget(self: &Arc<Self>, gid: &str, minutes: u32, ratio: f32, leftovers: Vec<PathBuf>) {
        self.heads.lock_ok().remove(gid);
        let turn = self.turns.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        self.sharing.lock_ok().insert(gid.to_string(), turn);
        let me = self.clone();
        let gid = gid.to_string();
        tokio::spawn(async move {
            let started = Instant::now();
            loop {
                let ours = me.sharing.lock_ok().get(&gid) == Some(&turn);
                if !ours {
                    return; // started again since: not ours to end
                }
                let done = match me.handle(&gid).await {
                    Ok(h) => {
                        let s = h.stats();
                        let sent = s.uploaded_bytes as f64 / s.total_bytes.max(1) as f64;
                        (minutes == 0 && ratio <= 0.0)
                            || (minutes > 0 && started.elapsed() >= Duration::from_secs(minutes as u64 * 60))
                            || (ratio > 0.0 && sent >= ratio as f64)
                            || matches!(s.state, TorrentStatsState::Paused | TorrentStatsState::Error)
                    }
                    Err(_) => true,
                };
                if done {
                    if me.sharing.lock_ok().get(&gid) == Some(&turn) {
                        me.forget(&gid).await;
                        for f in &leftovers {
                            if std::fs::remove_file(f).is_ok() {
                                if let Some(dir) = f.parent() {
                                    let _ = std::fs::remove_dir(dir); // only when empty
                                }
                            }
                        }
                    }
                    return;
                }
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_choices_and_ids() {
        assert_eq!(parse_select("1,3-5", 10), vec![0, 2, 3, 4]);
        assert_eq!(parse_select("4-2, 9, 12, x, 0", 10), vec![8]);
        assert_eq!(parse_select("2,2,1", 3), vec![0, 1]);
        let m = "magnet:?xt=urn:btih:0123456789ABCDEF0123456789abcdef01234567&dn=x";
        assert_eq!(hash_of(m).as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
        assert_eq!(hash_of("magnet:?xt=urn:btih:zz"), None);
        // the same id written in base32
        let b32 = "magnet:?xt=urn:btih:aerukz4jvpg66ajdivtytk6n54asgrlh&dn=x";
        assert_eq!(hash_of(b32).as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
    }
}
