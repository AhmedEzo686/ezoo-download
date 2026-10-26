//! Torrents (.torrent files and magnet links).
//!
//! The sharing itself is done by aria2 (`aria2c.exe`, a small, well known
//! open-source download program that ships with Ezoo Download in the tools
//! folder). It runs hidden, only while Ezoo Download runs, and only answers
//! on this computer (127.0.0.1, with a random password per start). Ezoo
//! Download adds, watches, stops and removes the torrents through its
//! JSON-RPC interface and shows them like every other download.
//!
//! By default nothing is shared (seeded) after a torrent is complete; the
//! user can choose minutes of sharing in Options > Torrents.

use anyhow::{anyhow, bail, Context};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

// ------------------------------------------------------------------ bencode

/// A value of a .torrent file (bencode).
#[derive(Debug, Clone, PartialEq)]
enum Ben {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Ben>),
    Dict(Vec<(Vec<u8>, Ben)>),
}

impl Ben {
    fn get(&self, key: &str) -> Option<&Ben> {
        match self {
            Ben::Dict(d) => d.iter().find(|(k, _)| k == key.as_bytes()).map(|(_, v)| v),
            _ => None,
        }
    }
    fn int(&self) -> Option<i64> {
        match self {
            Ben::Int(i) => Some(*i),
            _ => None,
        }
    }
    fn text(&self) -> Option<String> {
        match self {
            Ben::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        }
    }
    fn list(&self) -> &[Ben] {
        match self {
            Ben::List(l) => l,
            _ => &[],
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    depth: u32,
}

impl Reader<'_> {
    fn peek(&self) -> anyhow::Result<u8> {
        self.data.get(self.pos).copied().ok_or_else(|| anyhow!("the torrent file is cut short"))
    }
    fn number_until(&mut self, end: u8) -> anyhow::Result<i64> {
        let start = self.pos;
        while self.peek()? != end {
            self.pos += 1;
            if self.pos - start > 20 {
                bail!("not a torrent file");
            }
        }
        let s = std::str::from_utf8(&self.data[start..self.pos]).map_err(|_| anyhow!("not a torrent file"))?;
        self.pos += 1;
        s.parse().map_err(|_| anyhow!("not a torrent file"))
    }
    fn value(&mut self) -> anyhow::Result<Ben> {
        self.depth += 1;
        if self.depth > 64 {
            bail!("not a torrent file");
        }
        let v = match self.peek()? {
            b'i' => {
                self.pos += 1;
                Ben::Int(self.number_until(b'e')?)
            }
            b'l' => {
                self.pos += 1;
                let mut list = Vec::new();
                while self.peek()? != b'e' {
                    list.push(self.value()?);
                }
                self.pos += 1;
                Ben::List(list)
            }
            b'd' => {
                self.pos += 1;
                let mut dict = Vec::new();
                while self.peek()? != b'e' {
                    let Ben::Bytes(k) = self.value()? else { bail!("not a torrent file") };
                    let v = self.value()?;
                    dict.push((k, v));
                }
                self.pos += 1;
                Ben::Dict(dict)
            }
            b'0'..=b'9' => {
                let len = usize::try_from(self.number_until(b':')?).map_err(|_| anyhow!("not a torrent file"))?;
                let end = self.pos.checked_add(len).filter(|e| *e <= self.data.len()).ok_or_else(|| anyhow!("the torrent file is cut short"))?;
                let b = self.data[self.pos..end].to_vec();
                self.pos = end;
                Ben::Bytes(b)
            }
            _ => bail!("not a torrent file"),
        };
        self.depth -= 1;
        Ok(v)
    }
}

/// One file inside a torrent.
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentFile {
    /// path inside the torrent's folder, with "/" between the parts
    pub path: String,
    pub size: u64,
    /// its number in the torrent, 1-based, counting the padding files the
    /// list leaves out (what "select-file" wants)
    pub index: usize,
}

/// What a .torrent file shares.
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentInfo {
    /// the folder (several files) or file name
    pub name: String,
    pub files: Vec<TorrentFile>,
    pub total: u64,
}

/// Read a .torrent file.
pub fn parse_torrent(data: &[u8]) -> anyhow::Result<TorrentInfo> {
    let root = Reader { data, pos: 0, depth: 0 }.value()?;
    let info = root.get("info").ok_or_else(|| anyhow!("not a torrent file"))?;
    let name = info
        .get("name.utf-8")
        .or_else(|| info.get("name"))
        .and_then(Ben::text)
        .unwrap_or_else(|| "torrent".into());
    let mut files = Vec::new();
    if let Some(list) = info.get("files") {
        for (n, f) in list.list().iter().enumerate() {
            let parts = f.get("path.utf-8").or_else(|| f.get("path")).map(Ben::list).unwrap_or(&[]);
            let path: Vec<String> = parts.iter().filter_map(Ben::text).collect();
            // padding files (BEP 47) are not real files
            let pad = f.get("attr").and_then(Ben::text).map(|a| a.contains('p')).unwrap_or(false)
                || path.first().map(|p| p.starts_with(".pad")).unwrap_or(false);
            if pad {
                continue;
            }
            files.push(TorrentFile {
                path: path.join("/"),
                size: f.get("length").and_then(Ben::int).unwrap_or(0).max(0) as u64,
                index: n + 1,
            });
        }
    } else {
        files.push(TorrentFile { path: name.clone(), size: info.get("length").and_then(Ben::int).unwrap_or(0).max(0) as u64, index: 1 });
    }
    if files.is_empty() {
        bail!("this torrent has no files");
    }
    let total = files.iter().map(|f| f.size).sum();
    Ok(TorrentInfo { name, files, total })
}

/// A magnet link of a torrent ("magnet:?xt=urn:btih:...").
pub fn is_magnet(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    // a real one is a few hundred characters
    s.len() <= 8192 && !s.contains(char::is_whitespace) && s.starts_with("magnet:?") && (s.contains("xt=urn:btih:") || s.contains("xt=urn:btmh:"))
}

/// Is this download a torrent: a magnet link or a .torrent file on the disk.
pub fn is_torrent_source(s: &str) -> bool {
    is_magnet(s) || (!s.contains("://") && s.to_ascii_lowercase().ends_with(".torrent"))
}

/// The name a magnet link gives ("dn="), if any.
pub fn magnet_name(uri: &str) -> Option<String> {
    let query = uri.trim().split_once('?')?.1;
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == "dn").then(|| percent_encoding::percent_decode_str(&v.replace('+', " ")).decode_utf8_lossy().trim().to_string())
    }).filter(|n| !n.is_empty())
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

// --------------------------------------------------------------------- aria2

struct Daemon {
    port: u16,
    secret: String,
    child: std::process::Child,
}

/// The hidden aria2 that shares the torrents (started when first needed).
pub struct Aria2 {
    client: reqwest::Client,
    daemon: tokio::sync::Mutex<Option<Daemon>>,
    /// folder for aria2's own files (the DHT list)
    data_dir: PathBuf,
}

/// The state of one torrent, as aria2 reports it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TorrentStatus {
    /// "active", "waiting", "paused", "error", "complete", "removed"
    pub status: String,
    pub total: u64,
    pub done: u64,
    pub speed: u64,
    pub upload_speed: u64,
    pub connections: u32,
    pub seeders: u32,
    /// all chosen files are here (it may still share them)
    pub seeding: bool,
    /// the torrent's name, once known (magnet links learn it first)
    pub name: Option<String>,
    /// a magnet link got its file list: the torrent itself goes on here
    pub followed_by: Option<String>,
    pub error: Option<String>,
}

fn num(v: &Value) -> u64 {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64()).unwrap_or(0)
}

/// Options for one torrent.
#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub dir: PathBuf,
    /// files to get, 1-based ("1,3-5"); `None` = all
    pub select: Option<String>,
    /// minutes of sharing after it is complete (0 = none)
    pub seed_minutes: u32,
    /// KB/s, 0 = no limit
    pub download_limit_kb: u64,
    pub upload_limit_kb: u64,
    /// check the pieces already on the disk first
    pub check: bool,
}

impl Aria2 {
    pub fn new(data_dir: &Path) -> Aria2 {
        // straight to 127.0.0.1, never through a proxy
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(20)).build().unwrap_or_default();
        Aria2 { client, daemon: tokio::sync::Mutex::new(None), data_dir: data_dir.to_path_buf() }
    }

    async fn ensure(&self) -> anyhow::Result<(u16, String)> {
        let mut guard = self.daemon.lock().await;
        if let Some(d) = guard.as_mut() {
            if matches!(d.child.try_wait(), Ok(None)) {
                return Ok((d.port, d.secret.clone()));
            }
        }
        let tool = crate::mux::find_tool("aria2c").ok_or_else(|| {
            anyhow!(crate::tools::missing_tool_message("aria2c"))
        })?;
        let port = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let secret = random_secret();
        let _ = std::fs::create_dir_all(&self.data_dir);
        let dht = self.data_dir.join("dht.dat");
        let mut cmd = std::process::Command::new(&tool);
        cmd.args([
            "--enable-rpc=true",
            "--rpc-listen-all=false",
            "--rpc-allow-origin-all=false",
            "--rpc-max-request-size=32M",
            "--continue=true",
            "--file-allocation=none",
            "--seed-time=0",
            "--bt-save-metadata=false",
            "--follow-torrent=mem",
            "--follow-metalink=false",
            "--auto-file-renaming=false",
            "--allow-overwrite=false",
            "--max-concurrent-downloads=64",
            "--enable-dht=true",
            "--bt-enable-lpd=true",
            "--enable-peer-exchange=true",
            "--bt-max-peers=80",
            "--console-log-level=error",
            "--summary-interval=0",
            "--quiet=true",
            "--check-certificate=true",
            "--disk-cache=32M",
        ])
        .arg(format!("--rpc-listen-port={port}"))
        .arg(format!("--rpc-secret={secret}"))
        .arg(format!("--stop-with-process={}", std::process::id()))
        .arg(format!("--dht-file-path={}", dht.display()))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
        crate::mux::hide_window(&mut cmd);
        let child = cmd.spawn().context("could not start aria2c")?;
        *guard = Some(Daemon { port, secret: secret.clone(), child });
        drop(guard);
        // ready when it answers
        for _ in 0..50 {
            if self.raw_call(port, &secret, "aria2.getVersion", vec![]).await.is_ok() {
                return Ok((port, secret));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        bail!("aria2c did not start")
    }

    async fn raw_call(&self, port: u16, secret: &str, method: &str, mut params: Vec<Value>) -> anyhow::Result<Value> {
        params.insert(0, json!(format!("token:{secret}")));
        let body = json!({ "jsonrpc": "2.0", "id": "ezoo", "method": method, "params": params });
        let bytes = self
            .client
            .post(format!("http://127.0.0.1:{port}/jsonrpc"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?
            .bytes()
            .await?;
        let r: Value = serde_json::from_slice(&bytes).context("aria2 answered something unexpected")?;
        if let Some(e) = r.get("error") {
            bail!("{}", e["message"].as_str().unwrap_or("aria2 error"));
        }
        Ok(r["result"].clone())
    }

    async fn call(&self, method: &str, params: Vec<Value>) -> anyhow::Result<Value> {
        let (port, secret) = self.ensure().await?;
        self.raw_call(port, &secret, method, params).await
    }

    /// Start a torrent; gives aria2's id for it.
    pub async fn add(&self, source: &str, o: &AddOptions) -> anyhow::Result<String> {
        let mut opts = json!({
            "dir": o.dir.to_string_lossy(),
            "seed-time": o.seed_minutes.to_string(),
            "max-download-limit": format!("{}K", o.download_limit_kb),
            "max-upload-limit": format!("{}K", o.upload_limit_kb),
            "bt-remove-unselected-file": "true",
            "pause-metadata": "false",
        });
        if o.check {
            opts["check-integrity"] = json!("true");
        }
        if let Some(sel) = o.select.as_deref().filter(|s| !s.is_empty()) {
            opts["select-file"] = json!(sel);
        }
        let gid = if is_magnet(source) {
            self.call("aria2.addUri", vec![json!([source.trim()]), opts]).await?
        } else {
            let data = std::fs::read(source).context("the .torrent file can't be read")?;
            parse_torrent(&data)?;
            self.call("aria2.addTorrent", vec![json!(base64(&data)), json!([]), opts]).await?
        };
        gid.as_str().map(String::from).ok_or_else(|| anyhow!("aria2 did not take the torrent"))
    }

    pub async fn status(&self, gid: &str) -> anyhow::Result<TorrentStatus> {
        let keys = json!([
            "status", "totalLength", "completedLength", "downloadSpeed", "uploadSpeed", "connections",
            "numSeeders", "seeder", "bittorrent", "followedBy", "errorMessage", "errorCode"
        ]);
        let r = self.call("aria2.tellStatus", vec![json!(gid), keys]).await?;
        let name = r["bittorrent"]["info"]["name"].as_str().map(String::from).filter(|n| !n.is_empty());
        Ok(TorrentStatus {
            status: r["status"].as_str().unwrap_or("").to_string(),
            total: num(&r["totalLength"]),
            done: num(&r["completedLength"]),
            speed: num(&r["downloadSpeed"]),
            upload_speed: num(&r["uploadSpeed"]),
            connections: num(&r["connections"]) as u32,
            seeders: num(&r["numSeeders"]) as u32,
            seeding: r["seeder"].as_str() == Some("true"),
            name,
            followed_by: r["followedBy"].as_array().and_then(|a| a.first()).and_then(|g| g.as_str()).map(String::from),
            error: r["errorMessage"].as_str().filter(|m| !m.is_empty()).map(String::from),
        })
    }

    /// Stop it now (what is here stays, it goes on from there next time).
    pub async fn remove(&self, gid: &str) {
        if self.daemon.lock().await.is_none() {
            return;
        }
        let _ = self.call("aria2.forceRemove", vec![json!(gid)]).await;
        // forget it, so the same torrent can be added again
        for _ in 0..20 {
            if self.call("aria2.removeDownloadResult", vec![json!(gid)]).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// The files of a torrent once aria2 knows them, with how much of each
    /// is here.
    pub async fn files(&self, gid: &str) -> anyhow::Result<Vec<FileState>> {
        let r = self.call("aria2.getFiles", vec![json!(gid)]).await?;
        Ok(r.as_array()
            .map(|a| {
                a.iter()
                    .map(|f| FileState {
                        path: PathBuf::from(f["path"].as_str().unwrap_or("")),
                        size: num(&f["length"]),
                        done: num(&f["completedLength"]),
                        selected: f["selected"].as_str() == Some("true"),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// One file of a running torrent.
#[derive(Debug, Clone, PartialEq)]
pub struct FileState {
    pub path: PathBuf,
    pub size: u64,
    pub done: u64,
    /// one of the files the user wants (the others only get the bits of
    /// pieces they share with those)
    pub selected: bool,
}

impl Drop for Aria2 {
    fn drop(&mut self) {
        if let Some(d) = self.daemon.get_mut().as_mut() {
            let _ = d.child.kill();
        }
    }
}

fn random_secret() -> String {
    use ring::rand::SecureRandom;
    let mut b = [0u8; 16];
    if ring::rand::SystemRandom::new().fill(&mut b).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        b = (t ^ u128::from(std::process::id())).to_le_bytes();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ben_str(s: &str) -> String {
        format!("{}:{s}", s.len())
    }

    #[test]
    fn reads_a_multi_file_torrent() {
        let t = format!(
            "d8:announce{}4:infod5:filesld6:lengthi100e4:pathl{}{}eed6:lengthi5e4:pathl{}eed4:attr1:p6:lengthi7e4:pathl{}eee4:name{}12:piece lengthi16384e6:pieces0:ee",
            ben_str("http://t/a"),
            ben_str("Season 1"),
            ben_str("ep1.mkv"),
            ben_str("readme.txt"),
            ben_str(".pad"),
            ben_str("My Show")
        );
        let i = parse_torrent(t.as_bytes()).unwrap();
        assert_eq!(i.name, "My Show");
        assert_eq!(i.files, vec![
            TorrentFile { path: "Season 1/ep1.mkv".into(), size: 100, index: 1 },
            TorrentFile { path: "readme.txt".into(), size: 5, index: 2 },
        ]);
        assert_eq!(i.total, 105);
    }

    #[test]
    fn reads_a_single_file_torrent_and_refuses_junk() {
        let t = format!("d4:infod6:lengthi42e4:name{}ee", ben_str("film.mp4"));
        let i = parse_torrent(t.as_bytes()).unwrap();
        assert_eq!(i.files, vec![TorrentFile { path: "film.mp4".into(), size: 42, index: 1 }]);
        assert!(parse_torrent(b"<html>").is_err());
        assert!(parse_torrent(b"d4:infod6:lengthi42e4:name99:x").is_err());
        assert!(parse_torrent(&b"l".repeat(200)).is_err());
    }

    #[test]
    fn magnet_links() {
        let m = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Big+Film%20(2024)&tr=udp%3A%2F%2Ft";
        assert!(is_magnet(m));
        assert!(is_torrent_source(m));
        assert!(is_torrent_source(r"C:\Users\a\Downloads\x.torrent"));
        assert!(!is_torrent_source("https://site/x.torrent"));
        assert!(!is_magnet("magnet:?xt=urn:sha1:abc"));
        assert_eq!(magnet_name(m).as_deref(), Some("Big Film (2024)"));
        assert_eq!(magnet_name("magnet:?xt=urn:btih:00"), None);
    }

    #[test]
    fn base64_like_everyone() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
