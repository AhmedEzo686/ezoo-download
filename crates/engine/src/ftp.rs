//! Files on FTP servers (ftp://) and FTP over TLS (ftps://): the size and
//! date asked, then the file in parts on several connections when the
//! server lets a transfer start in the middle ("REST"), as HTTP downloads.
//!
//! ftp:// is plain FTP; when the server asks for TLS first it is switched on
//! ("AUTH TLS"). ftps:// is TLS from the first byte (port 990), or "AUTH TLS"
//! when the address names port 21.

use crate::http::{reconnect_delay, ConnGuard, NetDrop, Progress, Shared};
use crate::segments::{self, Segment};
use anyhow::{anyhow, bail, Context};
use std::fs::{File, OpenOptions};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

/// How long the server may take to answer a command, or to send more data.
const ANSWER: Duration = Duration::from_secs(30);
/// The longest line of an answer read (a server that never ends one is cut).
const MAX_LINE: usize = 8192;

pub fn is_ftp(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    u.starts_with("ftp://") || u.starts_with("ftps://")
}

/// A connection that is plain or over TLS.
trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// Where and who: from the address (and the download's login, if any).
#[derive(Debug, Clone)]
pub struct Site {
    host: String,
    port: u16,
    user: String,
    password: String,
    /// TLS from the first byte (ftps:// on its own port)
    implicit: bool,
    /// TLS asked for ("AUTH TLS"): ftps:// on port 21
    explicit: bool,
    /// the file's path as the address gives it (decoded)
    path: String,
}

impl Site {
    pub fn new(url: &str, auth: Option<&(String, String)>) -> anyhow::Result<Site> {
        let u = url::Url::parse(url.trim()).context("not a valid FTP address")?;
        let tls = match u.scheme() {
            "ftp" => false,
            "ftps" => true,
            _ => bail!("not an FTP address"),
        };
        let host = u.host_str().filter(|h| !h.is_empty()).ok_or_else(|| anyhow!("the FTP address names no server"))?.to_string();
        let port = u.port().unwrap_or(if tls { 990 } else { 21 });
        let decode = |s: &str| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned();
        let (user, password) = match auth.filter(|(u, _)| !u.is_empty()) {
            Some((u, p)) => (u.clone(), p.clone()),
            None if !u.username().is_empty() => (decode(u.username()), decode(u.password().unwrap_or(""))),
            None => ("anonymous".to_string(), "anonymous@".to_string()),
        };
        let path = decode(u.path());
        if path.trim_matches('/').is_empty() {
            bail!("the FTP address names no file");
        }
        if path.contains(['\r', '\n']) || user.contains(['\r', '\n']) || password.contains(['\r', '\n']) {
            bail!("not a valid FTP address");
        }
        Ok(Site { host, port, user, password, implicit: tls && port != 21, explicit: tls && port == 21, path })
    }

    /// The address without its password (what is kept in the list).
    pub fn without_password(url: &str) -> String {
        match url::Url::parse(url) {
            Ok(mut u) => {
                let _ = u.set_password(None);
                let _ = u.set_username("");
                u.to_string()
            }
            Err(_) => url.to_string(),
        }
    }

    /// The login an address carries ("ftp://user:pass@host/..."), if any.
    pub fn login_in(url: &str) -> Option<(String, String)> {
        let u = url::Url::parse(url).ok()?;
        let decode = |s: &str| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned();
        (!u.username().is_empty()).then(|| (decode(u.username()), decode(u.password().unwrap_or(""))))
    }
}

/// One FTP control connection, logged in.
struct Control {
    io: BufReader<Box<dyn Io>>,
    peer: SocketAddr,
    tls: bool,
    host: String,
}

/// The server's answer: its code and text.
struct Reply {
    code: u16,
    text: String,
}

/// An answer that is not the expected one.
#[derive(Debug)]
pub struct FtpReply {
    pub code: u16,
    pub text: String,
}
impl std::fmt::Display for FtpReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the FTP server answered {} {}", self.code, self.text.trim())
    }
}
impl std::error::Error for FtpReply {}

async fn timed<T>(f: impl std::future::Future<Output = std::io::Result<T>>) -> anyhow::Result<T> {
    match tokio::time::timeout(ANSWER, f).await {
        Ok(r) => Ok(r?),
        Err(_) => Err(NetDrop("the FTP server did not answer (timed out)").into()),
    }
}

#[cfg(windows)]
async fn tls(stream: Box<dyn Io>, host: &str) -> anyhow::Result<Box<dyn Io>> {
    let cx = native_tls::TlsConnector::builder().build().context("TLS could not be started")?;
    let cx = tokio_native_tls::TlsConnector::from(cx);
    let s = tokio::time::timeout(ANSWER, cx.connect(host, stream))
        .await
        .map_err(|_| NetDrop("the FTP server did not answer (timed out)"))?
        .context("the secure connection to the FTP server failed")?;
    Ok(Box::new(s))
}

#[cfg(not(windows))]
async fn tls(_stream: Box<dyn Io>, _host: &str) -> anyhow::Result<Box<dyn Io>> {
    bail!("FTP over TLS is not available here")
}

impl Control {
    async fn connect(site: &Site) -> anyhow::Result<Control> {
        let tcp = timed(TcpStream::connect((site.host.as_str(), site.port))).await.context("could not connect to the FTP server")?;
        let _ = tcp.set_nodelay(true);
        let peer = tcp.peer_addr()?;
        let mut io: Box<dyn Io> = Box::new(tcp);
        if site.implicit {
            io = tls(io, &site.host).await?;
        }
        let mut c = Control { io: BufReader::new(io), peer, tls: site.implicit, host: site.host.clone() };
        let hello = c.reply().await?;
        if hello.code != 220 {
            return Err(FtpReply { code: hello.code, text: hello.text }.into());
        }
        if site.explicit {
            c.secure().await?;
        }
        let mut user = c.cmd(&format!("USER {}", site.user)).await?;
        // the server wants TLS before the login
        if !c.tls && matches!(user.code, 530 | 534 | 550) && user.text.to_ascii_lowercase().contains("tls") {
            c.secure().await?;
            user = c.cmd(&format!("USER {}", site.user)).await?;
        }
        let logged = match user.code {
            230 => true,
            331 | 332 => c.cmd(&format!("PASS {}", site.password)).await?.code == 230,
            _ => false,
        };
        if !logged {
            bail!("the FTP server did not accept the user name or password - use \"Use authorization\" or Options > Site Logins");
        }
        if c.tls {
            c.cmd("PBSZ 0").await?;
            let prot = c.cmd("PROT P").await?;
            if prot.code != 200 {
                return Err(FtpReply { code: prot.code, text: prot.text }.into());
            }
        }
        let ty = c.cmd("TYPE I").await?;
        if ty.code != 200 {
            return Err(FtpReply { code: ty.code, text: ty.text }.into());
        }
        Ok(c)
    }

    /// "AUTH TLS", then everything over TLS.
    async fn secure(&mut self) -> anyhow::Result<()> {
        let r = self.cmd("AUTH TLS").await?;
        if r.code != 234 {
            bail!("the FTP server does not offer a secure connection ({} {})", r.code, r.text.trim());
        }
        // nothing is left unread in the buffer after "234"
        let plain = std::mem::replace(&mut self.io, BufReader::new(Box::new(tokio::io::duplex(1).0)));
        self.io = BufReader::new(tls(plain.into_inner(), &self.host).await?);
        self.tls = true;
        Ok(())
    }

    async fn line(&mut self) -> anyhow::Result<String> {
        let mut buf = Vec::new();
        let n = timed((&mut self.io).take(MAX_LINE as u64).read_until(b'\n', &mut buf)).await?;
        if n == 0 {
            return Err(NetDrop("the FTP server closed the connection").into());
        }
        Ok(String::from_utf8_lossy(&buf).trim_end().to_string())
    }

    async fn reply(&mut self) -> anyhow::Result<Reply> {
        let first = self.line().await?;
        let code: u16 = first.get(..3).and_then(|c| c.parse().ok()).ok_or_else(|| anyhow!("the FTP server sent something else: {first}"))?;
        let mut text = first.get(4..).unwrap_or("").to_string();
        // "123-..." goes on until a line "123 ..."
        if first.as_bytes().get(3) == Some(&b'-') {
            for _ in 0..1000 {
                let l = self.line().await?;
                text.push('\n');
                text.push_str(&l);
                if l.len() >= 4 && l.as_bytes()[3] == b' ' && l.starts_with(&first[..3]) {
                    break;
                }
            }
        }
        Ok(Reply { code, text })
    }

    async fn cmd(&mut self, c: &str) -> anyhow::Result<Reply> {
        timed(self.io.get_mut().write_all(format!("{c}\r\n").as_bytes())).await?;
        timed(self.io.get_mut().flush()).await?;
        self.reply().await
    }

    /// A data connection ("EPSV", else "PASV"): always to the server this
    /// connection talks to, whatever address the answer names.
    async fn data(&mut self) -> anyhow::Result<Box<dyn Io>> {
        let r = self.cmd("EPSV").await?;
        let port = if r.code == 229 {
            r.text.split('|').filter_map(|p| p.trim().parse::<u16>().ok()).next_back()
        } else {
            let r = self.cmd("PASV").await?;
            if r.code != 227 {
                return Err(FtpReply { code: r.code, text: r.text }.into());
            }
            let nums: Vec<u16> = r.text.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).collect();
            // "227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)"
            nums.len().checked_sub(2).and_then(|i| (nums[i] < 256 && nums[i + 1] < 256).then(|| nums[i] * 256 + nums[i + 1]))
        };
        let port = port.filter(|p| *p != 0).ok_or_else(|| anyhow!("the FTP server gave no data connection"))?;
        let tcp = timed(TcpStream::connect((self.peer.ip(), port))).await.context("could not open the FTP data connection")?;
        let io: Box<dyn Io> = Box::new(tcp);
        if self.tls {
            return tls(io, &self.host).await;
        }
        Ok(io)
    }

    /// The file from byte `at` on: its data connection.
    async fn open(&mut self, path: &str, at: u64) -> anyhow::Result<Box<dyn Io>> {
        let data = self.data().await?;
        if at > 0 {
            let r = self.cmd(&format!("REST {at}")).await?;
            if r.code != 350 {
                return Err(FtpReply { code: r.code, text: r.text }.into());
            }
        }
        let r = self.cmd(&format!("RETR {path}")).await?;
        if !matches!(r.code, 125 | 150) {
            return Err(FtpReply { code: r.code, text: r.text }.into());
        }
        Ok(data)
    }
}

/// What the server says about a file.
#[derive(Debug, Clone)]
pub struct Look {
    pub size: Option<u64>,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
    /// a transfer can start in the middle: parts, resume
    pub rest: bool,
    /// the path the server took (relative to where the login lands, or from the top)
    pub path: String,
}

/// The file's path as this server takes it: from where the login lands (as
/// the address means it), else from the top.
async fn find(c: &mut Control, site: &Site) -> anyhow::Result<(String, Option<u64>)> {
    let relative = site.path.trim_start_matches('/').to_string();
    let mut last = None;
    for p in [relative, site.path.clone()] {
        let r = c.cmd(&format!("SIZE {p}")).await?;
        if r.code == 213 {
            return Ok((p, r.text.trim().parse().ok()));
        }
        // 550: not there; 500-502: no SIZE (the file may still be there)
        if (500..=502).contains(&r.code) {
            return Ok((p, None));
        }
        last = Some(r);
    }
    let r = last.unwrap_or(Reply { code: 550, text: String::new() });
    if r.code == 550 {
        bail!("the file was not found on the FTP server (550)");
    }
    Err(FtpReply { code: r.code, text: r.text }.into())
}

pub async fn look(url: &str, auth: Option<&(String, String)>) -> anyhow::Result<Look> {
    let site = Site::new(url, auth)?;
    let mut c = Control::connect(&site).await?;
    let (path, size) = find(&mut c, &site).await?;
    let modified = c.cmd(&format!("MDTM {path}")).await.ok().filter(|r| r.code == 213).and_then(|r| {
        let t = r.text.trim();
        chrono::NaiveDateTime::parse_from_str(t.get(..14)?, "%Y%m%d%H%M%S").ok().map(|d| d.and_utc())
    });
    let rest = c.cmd("REST 0").await.map(|r| r.code == 350).unwrap_or(false);
    let _ = c.cmd("QUIT").await;
    Ok(Look { size, modified, rest, path })
}

/// The download's file name from its address.
pub fn file_name(url: &str) -> String {
    let path = url::Url::parse(url).map(|u| u.path().to_string()).unwrap_or_default();
    let last = path.rsplit('/').next().unwrap_or("");
    let name = percent_encoding::percent_decode_str(last).decode_utf8_lossy();
    let name = crate::filename::sanitize(&name);
    if name.is_empty() {
        "download".into()
    } else {
        name
    }
}

#[derive(Clone)]
pub struct FtpJob {
    pub url: String,
    pub auth: Option<(String, String)>,
    /// the path `look` found
    pub path: String,
    pub total_size: Option<u64>,
    pub rest: bool,
    pub part_path: PathBuf,
    pub connections: u32,
    pub min_split: u64,
    pub retries: u32,
    pub limiter: Arc<crate::limiter::SpeedLimiter>,
}

/// The whole file into `job.part_path`, in parts on several connections when
/// the server allows. `segs` is the parts table (kept for resuming).
pub async fn run(
    shared: Arc<Shared>,
    job: FtpJob,
    segs: Arc<Mutex<Vec<Segment>>>,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let site = Arc::new(Site::new(&job.url, job.auth.as_ref())?);
    if let Some(dir) = job.part_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let segmented = job.rest && job.total_size.is_some();
    let file = OpenOptions::new().create(true).read(true).write(true).truncate(false).open(&job.part_path)?;
    {
        let mut table = segs.lock().unwrap_or_else(|e| e.into_inner());
        if segmented {
            let total = job.total_size.unwrap_or(0);
            if table.is_empty() {
                *table = segments::plan(total, job.connections.max(1), job.min_split);
                file.set_len(0)?;
            }
            if file.metadata()?.len() != total {
                crate::fileio::make_sparse(&file);
                file.set_len(total)?;
            }
        } else {
            file.set_len(0)?;
            *table = vec![Segment::new(0, job.total_size.unwrap_or(u64::MAX))];
        }
        for s in table.iter_mut() {
            s.active = false;
            s.on_disk = Some(s.pos);
        }
        progress.bytes.store(segments::downloaded(&table), Ordering::Relaxed);
    }
    let file = Arc::new(file);
    let child = cancel.child_token();
    let wanted = if segmented { job.connections.clamp(1, 16) } else { 1 };
    let mut set = tokio::task::JoinSet::new();
    for n in 0..wanted {
        let w = Worker {
            shared: shared.clone(),
            job: job.clone(),
            site: site.clone(),
            segs: segs.clone(),
            progress: progress.clone(),
            file: file.clone(),
            cancel: child.clone(),
            segmented,
            first: n == 0,
        };
        set.spawn(w.work());
    }
    let mut first_error = None;
    let mut to_disk = tokio::time::interval(Duration::from_secs(4));
    to_disk.tick().await;
    loop {
        tokio::select! {
            r = set.join_next() => {
                let Some(r) = r else { break };
                match r.map_err(|e| anyhow!("worker crashed: {e}")).and_then(|r| r) {
                    Ok(()) => {}
                    Err(e) if e.is::<TooMany>() => {}
                    Err(e) => {
                        if first_error.is_none() && !cancel.is_cancelled() {
                            first_error = Some(e);
                        }
                        child.cancel();
                    }
                }
            }
            _ = to_disk.tick() => {
                let seen: Vec<u64> = segs.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|s| s.pos).collect();
                let f = file.clone();
                if tokio::task::spawn_blocking(move || f.sync_data().is_ok()).await.unwrap_or(false) {
                    for (s, pos) in segs.lock().unwrap_or_else(|e| e.into_inner()).iter_mut().zip(seen) {
                        s.on_disk = Some(pos.min(s.pos));
                    }
                }
            }
        }
    }
    if file.sync_all().is_ok() {
        for s in segs.lock().unwrap_or_else(|e| e.into_inner()).iter_mut() {
            s.on_disk = None;
        }
    }
    if cancel.is_cancelled() {
        return Err(crate::http_download::Cancelled.into());
    }
    if let Some(e) = first_error {
        return Err(e);
    }
    let table = segs.lock().unwrap_or_else(|e| e.into_inner());
    if segmented && !segments::all_finished(&table) {
        bail!("download ended before all parts were received");
    }
    Ok(())
}

/// The server allows fewer connections: this one stops, the others go on.
#[derive(Debug)]
struct TooMany;
impl std::fmt::Display for TooMany {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the FTP server allows fewer connections")
    }
}
impl std::error::Error for TooMany {}

struct Worker {
    shared: Arc<Shared>,
    job: FtpJob,
    site: Arc<Site>,
    segs: Arc<Mutex<Vec<Segment>>>,
    progress: Arc<Progress>,
    file: Arc<File>,
    cancel: CancellationToken,
    segmented: bool,
    /// the first connection: never the one that gives way
    first: bool,
}

impl Worker {
    fn table(&self) -> std::sync::MutexGuard<'_, Vec<Segment>> {
        self.segs.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn work(self) -> anyhow::Result<()> {
        loop {
            let idx = {
                let mut t = self.table();
                if self.segmented {
                    segments::claim_work(&mut t, self.job.min_split).or_else(|| segments::split_largest(&mut t, self.job.min_split.min(64 * 1024)))
                } else if t[0].active || t[0].is_finished() {
                    None
                } else {
                    t[0].active = true;
                    Some(0)
                }
            };
            let Some(idx) = idx else { return Ok(()) };
            let r = self.segment(idx).await;
            if let Some(s) = self.table().get_mut(idx) {
                s.active = false;
            }
            r?;
        }
    }

    async fn segment(&self, idx: usize) -> anyhow::Result<()> {
        let (mut fails, mut net) = (0u32, 0u32);
        loop {
            if self.cancel.is_cancelled() {
                return Err(crate::http_download::Cancelled.into());
            }
            let before = self.table()[idx].pos;
            let r = tokio::select! {
                r = self.fetch(idx) => r,
                _ = self.cancel.cancelled() => return Err(crate::http_download::Cancelled.into()),
            };
            let e = match r {
                Ok(()) if self.table()[idx].is_finished() => return Ok(()),
                Ok(()) => continue,
                Err(e) => e,
            };
            if self.table()[idx].pos > before {
                fails = 0;
                net = 0;
            }
            let code = e.chain().find_map(|c| c.downcast_ref::<FtpReply>().map(|r| r.code));
            // 421 / 530 "too many users": one connection less
            if !self.first && matches!(code, Some(421)) {
                return Err(TooMany.into());
            }
            let network = crate::http::is_network_error(&e) || matches!(code, Some(421 | 425 | 426 | 450));
            if network {
                net += 1;
                self.progress.waiting.store(true, Ordering::Relaxed);
            } else {
                fails += 1;
                if matches!(code, Some(550 | 553)) || fails > self.job.retries {
                    return Err(e);
                }
            }
            if !self.segmented {
                // no resume: from the start again
                let mut t = self.table();
                self.progress.bytes.fetch_sub(t[idx].done(), Ordering::Relaxed);
                t[idx].pos = t[idx].start;
                let _ = self.file.set_len(0);
            }
            let wait = if network { reconnect_delay(net) } else { Duration::from_secs((1u64 << fails.min(5)).min(30)) };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = self.progress.back_online.notified() => {}
                _ = self.cancel.cancelled() => return Err(crate::http_download::Cancelled.into()),
            }
        }
    }

    async fn fetch(&self, idx: usize) -> anyhow::Result<()> {
        let pos = self.table()[idx].pos;
        let mut c = Control::connect(&self.site).await?;
        let mut data = c.open(&self.job.path, pos).await?;
        let _guard = ConnGuard::new(&self.progress);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match tokio::time::timeout(ANSWER, data.read(&mut buf)).await {
                Ok(r) => r?,
                Err(_) => return Err(NetDrop("connection timed out").into()),
            };
            if n == 0 {
                break;
            }
            self.shared.limiter.acquire(n).await;
            self.job.limiter.acquire(n).await;
            let done = {
                let mut t = self.table();
                let s = &mut t[idx];
                let take = (n as u64).min(s.end.saturating_sub(s.pos)) as usize;
                if take > 0 {
                    crate::fileio::write_at(&self.file, &buf[..take], s.pos)?;
                    s.pos += take as u64;
                    self.progress.bytes.fetch_add(take as u64, Ordering::Relaxed);
                    self.progress.online();
                }
                s.pos >= s.end
            };
            if done {
                // the part is here (another connection has the rest): this
                // transfer is let go
                return Ok(());
            }
        }
        // the whole rest of the file came
        let finished = tokio::time::timeout(ANSWER, c.reply()).await;
        let mut t = self.table();
        let s = &mut t[idx];
        if !self.segmented && (self.job.total_size.is_none() || s.end == u64::MAX) {
            s.end = s.pos;
            return Ok(());
        }
        if s.pos >= s.end {
            return Ok(());
        }
        match finished {
            Ok(Ok(r)) if r.code >= 400 => Err(FtpReply { code: r.code, text: r.text }.into()),
            _ => Err(NetDrop("connection closed early").into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        let s = Site::new("ftp://ftp.test/pub/a%20b.iso", None).unwrap();
        assert_eq!((s.port, s.user.as_str(), s.path.as_str(), s.implicit, s.explicit), (21, "anonymous", "/pub/a b.iso", false, false));
        let s = Site::new("ftps://u%40x:p%3Aw@ftp.test/f.zip", None).unwrap();
        assert_eq!((s.port, s.user.as_str(), s.password.as_str(), s.implicit), (990, "u@x", "p:w", true));
        let s = Site::new("ftps://ftp.test:21/f.zip", Some(&("me".into(), "pw".into()))).unwrap();
        assert!(s.explicit && !s.implicit && s.user == "me");
        assert!(Site::new("ftp://ftp.test/", None).is_err());
        assert!(Site::new("ftp://ftp.test/a%0D%0ADELE%20x", None).is_err(), "no command smuggled in a path");
        assert_eq!(Site::without_password("ftp://u:p@h.test/f"), "ftp://h.test/f");
        assert_eq!(Site::login_in("ftp://u:p%21@h.test/f"), Some(("u".into(), "p!".into())));
        assert_eq!(file_name("ftp://h.test/dir/My%20File.zip"), "My File.zip");
        assert!(is_ftp("FTPS://x/y") && !is_ftp("https://x"));
    }
}
