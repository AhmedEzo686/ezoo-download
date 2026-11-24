//! Torrents (.torrent files and magnet links).
//!
//! Reading .torrent files and magnet links, and what the windows show of a
//! running torrent. The sharing itself is in [`crate::bt`] (librqbit, inside
//! the program).
//!
//! By default nothing is shared (seeded) after a torrent is complete; the
//! user can choose minutes of sharing in Options > Torrents.

use anyhow::{anyhow, bail};
use std::path::{Path, PathBuf};

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
    /// several files (in a folder named `name`), or one
    pub multi: bool,
    /// the trackers it names
    pub trackers: Vec<String>,
    /// its id (the SHA-1 of its "info" part, as a magnet link names it)
    pub info_hash: Option<String>,
}

/// More files than any real torrent has: refused (a file made to hurt).
const MAX_FILES: usize = 200_000;

/// A part of a path in a torrent as Windows can keep it safely: no "..",
/// no name of a device (CON, NUL...), none of \ / : * ? " < > |, no dot or
/// space at the end.
pub fn safe_part(part: &str) -> String {
    let p = part.trim();
    if p.is_empty() || p == "." || p == ".." {
        return "_".into();
    }
    let s = crate::filename::sanitize(p);
    if s.is_empty() || s.chars().all(|c| c == '.') {
        "_".into()
    } else {
        s
    }
}

/// The files whose path in the torrent is not safe to write as it is: their
/// number and the path to write them at instead (from the save folder, as
/// aria2's "index-out" wants it). Empty: every path is fine.
pub fn safe_paths(info: &TorrentInfo) -> Vec<(usize, String)> {
    let name = safe_part(&info.name);
    let name_changed = name != info.name;
    let mut out = Vec::new();
    for f in &info.files {
        let parts: Vec<&str> = f.path.split('/').collect();
        let safe: Vec<String> = parts.iter().map(|p| safe_part(p)).collect();
        let changed = name_changed || parts.iter().zip(&safe).any(|(a, b)| *a != b.as_str());
        if !changed {
            continue;
        }
        let rel = if info.multi { format!("{name}/{}", safe.join("/")) } else { name.clone() };
        out.push((f.index, rel));
    }
    out
}

/// A path written by the torrent program stays inside `dir`.
pub fn stays_inside(path: &Path, dir: &Path) -> bool {
    use std::path::Component;
    // "dir/./name" as aria2 writes it; a path given from the folder
    let clean = |p: &Path| p.components().filter(|c| !matches!(c, Component::CurDir)).collect::<PathBuf>();
    let (path, dir) = (clean(path), clean(dir));
    let rel = if path.is_absolute() {
        match path.strip_prefix(&dir) {
            Ok(r) => r.to_path_buf(),
            Err(_) => return false,
        }
    } else {
        path
    };
    rel.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Where the "info" part of a .torrent file is (its bytes make the id).
fn info_span(data: &[u8]) -> Option<(usize, usize)> {
    let mut r = Reader { data, pos: 0, depth: 0 };
    if r.peek().ok()? != b'd' {
        return None;
    }
    r.pos += 1;
    while r.peek().ok()? != b'e' {
        let Ben::Bytes(k) = r.value().ok()? else { return None };
        let start = r.pos;
        r.value().ok()?;
        if k == b"info" {
            return Some((start, r.pos));
        }
    }
    None
}

/// The magnet link of a .torrent file (its id, name and trackers).
pub fn magnet_of(data: &[u8]) -> Option<String> {
    let info = parse_torrent(data).ok()?;
    let hash = info.info_hash?;
    let enc = |s: &str| percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string();
    let mut m = format!("magnet:?xt=urn:btih:{hash}&dn={}", enc(&info.name));
    for t in info.trackers.iter().take(20) {
        m.push_str("&tr=");
        m.push_str(&enc(t));
    }
    Some(m)
}

/// Read a .torrent file.
pub fn parse_torrent(data: &[u8]) -> anyhow::Result<TorrentInfo> {
    // a description, never more (the biggest real ones are a few MB)
    if data.len() > 32 << 20 {
        bail!("not a torrent file");
    }
    let root = Reader { data, pos: 0, depth: 0 }.value()?;
    let info = root.get("info").ok_or_else(|| anyhow!("not a torrent file"))?;
    let name = info
        .get("name.utf-8")
        .or_else(|| info.get("name"))
        .and_then(Ben::text)
        .unwrap_or_else(|| "torrent".into());
    let mut files = Vec::new();
    let multi = info.get("files").is_some();
    if info.get("files").is_some_and(|l| l.list().len() > MAX_FILES) {
        bail!("this torrent has too many files");
    }
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
    let total = files.iter().fold(0u64, |a, f| a.saturating_add(f.size));
    let mut trackers: Vec<String> = Vec::new();
    for t in root.get("announce").and_then(Ben::text).into_iter().chain(
        root.get("announce-list").map(Ben::list).unwrap_or(&[]).iter().flat_map(|tier| tier.list().iter().filter_map(Ben::text)),
    ) {
        let t = t.trim().to_string();
        if !t.is_empty() && t.len() < 1024 && !trackers.contains(&t) {
            trackers.push(t);
        }
    }
    let info_hash = info_span(data).map(|(a, b)| {
        let d = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, &data[a..b]);
        d.as_ref().iter().map(|x| format!("{x:02x}")).collect()
    });
    Ok(TorrentInfo { name, files, total, multi, trackers, info_hash })
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

// --------------------------------------------------------------- running

/// The state of one torrent.
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
    /// how many pieces, and which are here (hex, a bit a piece)
    pub pieces: u32,
    pub bitfield: String,
    /// the trackers it asks
    pub trackers: Vec<String>,
    /// bytes sent to others
    pub upload_total: u64,
}

/// One person a torrent is connected to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Peer {
    pub address: String,
    pub down: u64,
    pub up: u64,
    /// how much of the torrent they have (0..1; below 0: not known)
    pub have: f32,
    pub seeder: bool,
}

/// How much of the pieces a bitfield (hex) says are there (0..1).
pub fn share_of(bitfield: &str) -> f32 {
    if bitfield.is_empty() {
        return 0.0;
    }
    let ones: u32 = bitfield.chars().filter_map(|c| c.to_digit(16)).map(|d| d.count_ones()).sum();
    ones as f32 / (bitfield.len() as f32 * 4.0)
}

/// Which pieces are here, as up to `n` runs: (from, to, here) in 0..1.
pub fn piece_runs(bitfield: &str, pieces: u32, n: usize) -> Vec<(f32, f32, bool)> {
    if pieces == 0 || bitfield.is_empty() || n == 0 {
        return Vec::new();
    }
    let has = |i: u32| -> bool {
        let c = bitfield.as_bytes().get((i / 4) as usize).and_then(|c| (*c as char).to_digit(16)).unwrap_or(0);
        c & (8 >> (i % 4)) != 0
    };
    let cells = (pieces as usize).min(n);
    let mut out: Vec<(f32, f32, bool)> = Vec::new();
    for cell in 0..cells {
        let a = (cell as u64 * pieces as u64 / cells as u64) as u32;
        let b = (((cell as u64 + 1) * pieces as u64 / cells as u64) as u32).max(a + 1);
        // a cell is "here" when all its pieces are
        let here = (a..b).all(has);
        let (from, to) = (cell as f32 / cells as f32, (cell + 1) as f32 / cells as f32);
        match out.last_mut() {
            Some(last) if last.2 == here => last.1 = to,
            _ => out.push((from, to, here)),
        }
    }
    out
}

/// Public trackers added to every torrent (the torrent's own come first).
/// Long-running, open ones that answer most torrents.
pub const PUBLIC_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://explodie.org:6969/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.dler.org:6969/announce",
    "udp://tracker.tiny-vps.com:6969/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
    "https://tracker.tamersunion.org:443/announce",
    "http://tracker.opentrackr.org:1337/announce",
];

/// Options for one torrent.
#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub dir: PathBuf,
    /// files to get, 1-based ("1,3-5"); `None` = all
    pub select: Option<String>,
    /// the download's own limit, KB/s (0 = only the limit of all downloads)
    pub download_limit_kb: u64,
    /// check every piece on the disk again (not what was noted of them)
    pub check: bool,
    /// the first and last pieces of the biggest file first (a video plays sooner)
    pub first_last: bool,
    /// trackers the user added (besides the torrent's own and the public ones)
    pub trackers: Vec<String>,
}

/// An address a tracker can have (udp, http, https; no spaces or commas).
pub fn is_tracker(t: &str) -> bool {
    let l = t.trim().to_ascii_lowercase();
    (l.starts_with("udp://") || l.starts_with("http://") || l.starts_with("https://"))
        && t.len() < 1024
        && !t.contains([',', ' ', '\n', '\r'])
        && url::Url::parse(t.trim()).is_ok()
}

/// One file of a running torrent.
#[derive(Debug, Clone, PartialEq)]
pub struct FileState {
    /// its number in the torrent (1-based)
    pub index: usize,
    pub path: PathBuf,
    pub size: u64,
    pub done: u64,
    /// one of the files the user wants (the others only get the bits of
    /// pieces they share with those)
    pub selected: bool,
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
        assert!(i.multi);
        assert_eq!(i.trackers, vec!["http://t/a".to_string()]);
        assert_eq!(i.info_hash.as_deref().map(str::len), Some(40));
        assert!(safe_paths(&i).is_empty(), "nothing to change");
    }

    #[test]
    fn unsafe_names_are_written_safely() {
        let t = format!(
            "d4:infod5:filesld6:lengthi1e4:pathl{}{}eed6:lengthi1e4:pathl{}eed6:lengthi1e4:pathl{}eee4:name{}ee",
            ben_str(".."),
            ben_str("evil.exe"),
            ben_str("CON"),
            ben_str("ok.txt"),
            ben_str("Show")
        );
        let i = parse_torrent(t.as_bytes()).unwrap();
        let out = safe_paths(&i);
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0], (1, "Show/_/evil.exe".to_string()));
        assert!(!out[1].1.ends_with("/CON"), "{out:?}");
        assert!(out.iter().all(|(_, p)| !p.split('/').any(|x| x == "..")));
        // a torrent whose own name leads out
        let t = format!("d4:infod6:lengthi3e4:name{}ee", ben_str("../../x.exe"));
        let i = parse_torrent(t.as_bytes()).unwrap();
        let out = safe_paths(&i);
        assert_eq!(out.len(), 1);
        assert!(!out[0].1.contains(['/', '\\']) && out[0].1 != "..", "a name, never a way out: {out:?}");
        assert!(stays_inside(Path::new("/d/a/b.txt"), Path::new("/d")));
        assert!(!stays_inside(Path::new("/d/../e.txt"), Path::new("/d")));
        assert!(!stays_inside(Path::new("/e/x"), Path::new("/d")));
        assert!(stays_inside(Path::new("/d/./a/b"), Path::new("/d/")));
    }

    #[test]
    fn magnets_trackers_and_pieces() {
        let t = format!("d8:announce{}4:infod6:lengthi3e4:name{}ee", ben_str("udp://t.test:1/a"), ben_str("a b"));
        let m = magnet_of(t.as_bytes()).unwrap();
        assert!(m.starts_with("magnet:?xt=urn:btih:") && m.contains("&dn=a%20b") && m.contains("&tr=udp%3A%2F%2Ft%2Etest%3A1%2Fa"), "{m}");
        assert!(is_magnet(&m));
        assert!(is_tracker("udp://tracker.test:80/announce"));
        assert!(!is_tracker("udp://a,b"));
        assert!(!is_tracker("file:///etc/passwd"));
        assert!((share_of("f0") - 0.5).abs() < 1e-6);
        // 8 pieces, the first 4 here
        let runs = piece_runs("f0", 8, 100);
        assert_eq!(runs, vec![(0.0, 0.5, true), (0.5, 1.0, false)]);
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

}
