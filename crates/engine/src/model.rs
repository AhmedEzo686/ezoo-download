//! Data types shared by the engine and the UI.

use crate::segments::Segment;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type DownloadId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Queued,
    Connecting,
    Downloading,
    Assembling,
    Paused,
    Completed,
    Failed,
}

impl Status {
    pub fn is_running(self) -> bool {
        matches!(self, Status::Connecting | Status::Downloading | Status::Assembling)
    }
    pub fn label(self) -> &'static str {
        match self {
            Status::Queued => "Queued",
            Status::Connecting => "Connecting…",
            Status::Downloading => "Downloading",
            Status::Assembling => "Assembling…",
            Status::Paused => "Paused",
            Status::Completed => "Complete",
            Status::Failed => "Error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadKind {
    /// Normal file over HTTP/HTTPS (segmented when the server allows ranges)
    Http,
    /// HLS stream (.m3u8): many small parts joined into one video file
    Hls,
    /// Separate video and audio streams (YouTube style, "DASH"):
    /// both are downloaded, then merged into one file with ffmpeg
    Dash,
    /// a .torrent file or a magnet link, shared with other computers
    Torrent,
}

/// What a download does after the data is all here (its second bar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// the picture and the sound into one file
    Join,
    /// a stream into an MP4 file
    ToMp4,
    /// the sound into an MP3 file
    ToMp3,
    /// the file to its folder on another disk
    Move,
    /// the part of a video cut out
    Cut,
    /// the finished file compared with its checksum
    Check,
}

impl Step {
    pub fn from_code(c: u8) -> Option<Step> {
        Some(match c {
            1 => Step::Join,
            2 => Step::ToMp4,
            3 => Step::ToMp3,
            4 => Step::Move,
            5 => Step::Cut,
            6 => Step::Check,
            _ => return None,
        })
    }
    pub fn code(self) -> u8 {
        match self {
            Step::Join => 1,
            Step::ToMp4 => 2,
            Step::ToMp3 => 3,
            Step::Move => 4,
            Step::Cut => 5,
            Step::Check => 6,
        }
    }
}

/// "Download a part of a video": only from `start` to `end` (seconds).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub start: f64,
    pub end: f64,
    /// cut on the very second asked (the start of the picture is made
    /// again: slower); otherwise the quick cut, which starts at the nearest
    /// whole picture before `start` (a second or two earlier, as it is)
    #[serde(default)]
    pub exact: bool,
}

impl Clip {
    /// The longest part (and video) taken: a day.
    pub const MAX: f64 = 24.0 * 3600.0;

    pub fn new(start: f64, end: f64, exact: bool) -> Option<Clip> {
        let ok = start.is_finite() && end.is_finite() && start >= 0.0 && end <= Clip::MAX && end - start >= 0.5;
        ok.then_some(Clip { start, end, exact })
    }
    pub fn len(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
    /// What goes into the file name: "01-20 → 03-45" (Windows takes no ':')
    pub fn label(&self) -> String {
        format!("{} → {}", clock(self.start, '-'), clock(self.end, '-'))
    }
}

/// 75.4 -> "01:15", 3725 -> "1:02:05" (`sep` between the numbers).
pub fn clock(secs: f64, sep: char) -> String {
    let t = secs.max(0.0).floor() as u64;
    let (h, m, s) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 {
        format!("{h}{sep}{m:02}{sep}{s:02}")
    } else {
        format!("{m:02}{sep}{s:02}")
    }
}

/// "1:02:05", "02:05", "125", "2:05.5" (Arabic digits too) -> seconds.
pub fn parse_clock(text: &str) -> Option<f64> {
    let t: String = text
        .trim()
        .chars()
        .map(|c| match c {
            '٠'..='٩' => char::from(b'0' + (c as u32 - '٠' as u32) as u8),
            '۰'..='۹' => char::from(b'0' + (c as u32 - '۰' as u32) as u8),
            '٫' | ',' => '.',
            '-' | '：' => ':',
            _ => c,
        })
        .collect();
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (t.as_str(), None),
    };
    let nums: Vec<&str> = whole.split(':').collect();
    if whole.is_empty() || nums.len() > 3 || nums.iter().any(|n| n.is_empty() || n.len() > 6 || !n.bytes().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let mut total = 0.0;
    for (i, n) in nums.iter().enumerate() {
        let v: f64 = n.parse().ok()?;
        // minutes and seconds after the first number stay under 60
        if i > 0 && v >= 60.0 {
            return None;
        }
        total = total * 60.0 + v;
    }
    if let Some(f) = frac {
        if f.is_empty() || f.len() > 3 || !f.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        total += format!("0.{f}").parse::<f64>().ok()?;
    }
    Some(total)
}

/// Extra request data, usually handed over by the browser extension so that
/// downloads behind a login work exactly like in the browser.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestHeaders {
    pub referer: Option<String>,
    /// the site's login cookies: encrypted on the disk (secret.rs)
    #[serde(default, with = "crate::secret::opt")]
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    #[serde(default)]
    pub extra: Vec<(String, String)>,
    /// user name + password for the server ("Use authorization")
    /// encrypted on the disk (secret.rs)
    #[serde(default, with = "crate::secret::pair")]
    pub auth: Option<(String, String)>,
    /// a form's download: what the form sends (base64), asked with POST;
    /// encrypted on the disk like cookies
    #[serde(default, with = "crate::secret::opt", skip_serializing_if = "Option::is_none")]
    pub post: Option<String>,
    /// the form's kind of data ("application/x-www-form-urlencoded"...)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_type: Option<String>,
}

/// What we learn about a URL before downloading it.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub final_url: String,
    pub file_name: String,
    pub total_size: Option<u64>,
    pub resumable: bool,
    pub content_type: Option<String>,
    pub kind: DownloadKind,
    /// the server's "Last-Modified" date
    pub last_modified: Option<DateTime<Utc>>,
    /// the server's name for this version of the file ("ETag")
    pub etag: Option<String>,
    /// the server refused the request as it was (HTTP 403 and the like) and
    /// took it with these headers instead: the download uses them too
    pub headers_used: Option<RequestHeaders>,
    /// the checksum the server sent with the file ("sha256:..."), if any
    pub digest: Option<String>,
}

impl ProbeResult {
    /// The address opens a web page, not a file.
    pub fn is_web_page(&self) -> bool {
        let ct = self.content_type.as_deref().unwrap_or("").to_ascii_lowercase();
        let ext = crate::filename::extension(&self.file_name);
        ct.starts_with("text/html") && !matches!(ext.as_str(), "htm" | "html" | "mht" | "mhtml")
    }
}

/// A request to add a download.
#[derive(Debug, Clone, Default)]
pub struct NewDownload {
    pub url: String,
    pub file_name: Option<String>,
    pub save_dir: Option<PathBuf>,
    pub headers: RequestHeaders,
    pub connections: Option<u32>,
    /// Result of an earlier probe (from the add dialog), if any
    pub probe: Option<ProbeResult>,
    /// Separate audio stream to merge with `url` (YouTube style downloads)
    pub audio_url: Option<String>,
    /// YouTube page + chosen quality, to get fresh links when old ones expire
    pub source_page: Option<String>,
    pub source_key: Option<String>,
    /// put it in this download queue
    pub queue: Option<u64>,
    /// the user's note from the download dialog
    pub description: Option<String>,
    /// a video page (playlist item): find its links when it starts, in this
    /// quality ("best", "audio", or a height like "720")
    pub pick_quality: Option<String>,
    /// started at once while the "Download File Info" window is open
    /// (its name and folder can change until the user confirms)
    pub early: bool,
    pub start: bool,
    /// torrents: the files to get, 1-based ("1,3,4"); None = all
    pub torrent_select: Option<String>,
    /// only this part of the video / sound
    pub clip: Option<Clip>,
    /// the checksum the finished file must have (what the user pasted)
    pub checksum: Option<String>,
}

/// Persisted state of one download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub id: DownloadId,
    pub url: String,
    pub final_url: Option<String>,
    pub file_name: String,
    pub save_dir: PathBuf,
    pub kind: DownloadKind,
    pub total_size: Option<u64>,
    pub resumable: bool,
    #[serde(default)]
    pub segments: Vec<Segment>,
    /// Dash downloads: the audio stream and its own segment table
    #[serde(default)]
    pub audio_url: Option<String>,
    #[serde(default)]
    pub audio_segments: Vec<Segment>,
    #[serde(default)]
    pub video_size: Option<u64>,
    #[serde(default)]
    pub source_page: Option<String>,
    #[serde(default)]
    pub source_key: Option<String>,
    #[serde(default)]
    pub queue: Option<u64>,
    /// name was only guessed from the URL; the server's real name wins
    #[serde(default)]
    pub auto_name: bool,
    #[serde(default)]
    pub description: Option<String>,
    /// this download's own speed limit in KB/s
    #[serde(default)]
    pub speed_limit_kb: Option<u64>,
    /// shut down / sleep / exit when this download finishes
    #[serde(default)]
    pub on_complete: crate::config::AfterDone,
    /// bytes done (used for HLS and for display after restart)
    #[serde(default)]
    pub downloaded: u64,
    #[serde(default)]
    pub parts_total: u32,
    #[serde(default)]
    pub parts_done: u32,
    pub status: Status,
    pub error: Option<String>,
    #[serde(default)]
    pub headers: RequestHeaders,
    pub connections: u32,
    pub category: String,
    pub added_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// temporary folder for the parts (Options > Save To); None = next to the file
    #[serde(default)]
    pub temp_dir: Option<PathBuf>,
    /// position inside its queue (Queue > Files in the queue: move up / down)
    #[serde(default)]
    pub queue_pos: u64,
    /// last time the download was started
    #[serde(default)]
    pub last_try: Option<DateTime<Utc>>,
    /// server's file date (Last-Modified)
    #[serde(default)]
    pub server_time: Option<DateTime<Utc>>,
    /// the server's name for the version of the file being downloaded
    /// ("ETag"): another one later = the file changed on the server
    #[serde(default)]
    pub etag: Option<String>,
    /// automatic retries done by its queue
    #[serde(default)]
    pub retry_count: u32,
    /// video page whose links are found when it starts (playlists)
    #[serde(default)]
    pub pick_quality: Option<String>,
    /// fixed name of the temporary files: the download started while the
    /// "Download File Info" window was still open, so its final name and
    /// folder may still change
    #[serde(default)]
    pub work_stem: Option<String>,
    /// torrents: the files to get, 1-based ("1,3,4"); None = all
    #[serde(default)]
    pub torrent_select: Option<String>,
    /// torrents: the files it writes, relative to `save_dir` (learnt while
    /// it runs; a magnet link only knows them then)
    #[serde(default)]
    pub torrent_files: Vec<String>,
    /// torrents: trackers the user added
    #[serde(default)]
    pub torrent_trackers: Vec<String>,
    /// torrents: the first and last pieces of each file first
    #[serde(default)]
    pub torrent_first_last: bool,
    /// torrents: check every piece on the disk at the next start
    #[serde(default)]
    pub torrent_recheck: bool,
    /// only this part of the video / sound ("Download a part")
    #[serde(default)]
    pub clip: Option<Clip>,
    /// the checksum the finished file must have ("sha256:..."): the user's,
    /// else the one the server sent
    #[serde(default)]
    pub checksum: Option<String>,
}

impl DownloadRecord {
    pub fn target_path(&self) -> PathBuf {
        self.save_dir.join(&self.file_name)
    }
    /// where the temporary files live
    pub fn work_dir(&self) -> PathBuf {
        match &self.temp_dir {
            Some(t) if !t.as_os_str().is_empty() => t.clone(),
            _ => self.save_dir.clone(),
        }
    }
    pub fn part_path(&self) -> PathBuf {
        if let Some(stem) = &self.work_stem {
            return self.work_dir().join(format!("{stem}.{}", crate::filename::PART_EXT));
        }
        match &self.temp_dir {
            // ids keep same-named files from different folders apart
            Some(t) if !t.as_os_str().is_empty() => {
                crate::filename::part_path(t, &format!("{}-{}", self.id, self.file_name))
            }
            _ => crate::filename::part_path(&self.save_dir, &self.file_name),
        }
    }
    /// base name of the temporary files (with the id when they share a temp folder)
    pub fn temp_stem(&self) -> String {
        if let Some(stem) = &self.work_stem {
            return stem.clone();
        }
        if self.work_dir() == self.save_dir {
            self.file_name.clone()
        } else {
            format!("{}-{}", self.id, self.file_name)
        }
    }
    pub fn hls_temp_dir(&self) -> PathBuf {
        self.work_dir().join(format!(".{}.sgparts", self.temp_stem()))
    }
    pub fn video_part_path(&self) -> PathBuf {
        self.work_dir().join(format!(".{}.video.sgpart", self.temp_stem()))
    }
    pub fn audio_part_path(&self) -> PathBuf {
        self.work_dir().join(format!(".{}.audio.sgpart", self.temp_stem()))
    }
    /// Torrents: every file it writes (only those: the torrent's folder may
    /// have the name of a folder that was there before).
    pub fn torrent_data_paths(&self) -> Vec<PathBuf> {
        if !self.torrent_files.is_empty() {
            return self.torrent_files.iter().map(|f| self.save_dir.join(f)).collect();
        }
        let Some(info) = std::fs::read(&self.url).ok().and_then(|d| crate::torrent::parse_torrent(&d).ok()) else {
            return Vec::new();
        };
        let single = info.files.len() == 1 && info.files[0].path == info.name;
        info.files
            .iter()
            .map(|f| {
                let rel: PathBuf = f.path.split('/').map(crate::filename::sanitize).collect();
                if single {
                    self.save_dir.join(&self.file_name)
                } else {
                    self.save_dir.join(&self.file_name).join(rel)
                }
            })
            .collect()
    }

    /// Every temporary file/folder this download may leave behind.
    pub fn temp_paths(&self) -> Vec<PathBuf> {
        if self.kind == DownloadKind::Torrent {
            // the notes an older version (aria2) kept of what it has
            return vec![self.save_dir.join(format!("{}.aria2", self.file_name))];
        }
        vec![self.part_path(), self.hls_temp_dir(), self.video_part_path(), self.audio_part_path()]
    }
    /// The file name of a part of a video: "name [01-20 → 03-45].mp4"
    /// (a picture is always an MP4 file: it plays everywhere).
    pub fn clip_name(name: &str, clip: &Clip) -> String {
        let (stem, ext) = crate::filename::split_ext(name);
        let ext = match ext.to_ascii_lowercase().as_str() {
            "mp3" => "mp3",
            "m4a" | "aac" => "m4a",
            "opus" | "ogg" => "ogg",
            _ => "mp4",
        };
        format!("{} [{}].{ext}", stem.trim(), clip.label())
    }
    pub fn request_url(&self) -> &str {
        self.final_url.as_deref().unwrap_or(&self.url)
    }
}

/// Snapshot of a download for the UI.
#[derive(Debug, Clone)]
pub struct DownloadView {
    pub id: DownloadId,
    pub url: String,
    pub file_name: String,
    pub save_path: PathBuf,
    pub category: String,
    pub kind: DownloadKind,
    pub status: Status,
    pub error: Option<String>,
    pub total_size: Option<u64>,
    pub downloaded: u64,
    /// bytes per second
    pub speed: f64,
    pub eta_secs: Option<u64>,
    pub active_connections: u32,
    /// torrents: connected people that have the whole file
    pub seeders: u32,
    pub resumable: bool,
    pub parts_total: u32,
    pub parts_done: u32,
    /// (start, pos, end, active) of every segment, for the connections view
    pub segments: Vec<(u64, u64, u64, bool)>,
    pub queue: Option<u64>,
    pub finished_at: Option<DateTime<Utc>>,
    pub description: Option<String>,
    /// the checksum the finished file must have ("sha256:...")
    pub checksum: Option<String>,
    pub speed_limit_kb: Option<u64>,
    pub on_complete: crate::config::AfterDone,
    pub added_at: DateTime<Utc>,
    pub last_try: Option<DateTime<Utc>>,
    pub headers: RequestHeaders,
    /// page the download came from
    pub parent_page: Option<String>,
    pub queue_pos: u64,
    /// the connection dropped: waiting for the internet to come back
    pub waiting: bool,
    /// after the data: what it does now and how far (0..1)
    pub step: Option<(Step, f32)>,
    /// video pages: the quality chosen ("1080p|MP4 AV1")
    pub source_key: Option<String>,
}

impl DownloadView {
    pub fn referer(&self) -> &str {
        self.headers.referer.as_deref().unwrap_or("")
    }
    pub fn progress(&self) -> f32 {
        if self.status == Status::Completed {
            return 1.0;
        }
        match self.total_size {
            Some(t) if t > 0 => (self.downloaded as f64 / t as f64).min(1.0) as f32,
            _ if self.parts_total > 0 => self.parts_done as f32 / self.parts_total as f32,
            _ => 0.0,
        }
    }
}

/// Human readable size: 1.5 MB
pub fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.2} {}", UNITS[u])
    }
}

pub fn fmt_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod clip_tests {
    use super::*;

    #[test]
    fn times_of_a_part() {
        assert_eq!(parse_clock("1:02:05"), Some(3725.0));
        assert_eq!(parse_clock("02:05"), Some(125.0));
        assert_eq!(parse_clock("125"), Some(125.0));
        assert_eq!(parse_clock("2:05.5"), Some(125.5));
        assert_eq!(parse_clock("٠١:٣٠"), Some(90.0));
        assert_eq!(parse_clock("1:75"), None);
        assert_eq!(parse_clock("abc"), None);
        assert_eq!(parse_clock(""), None);
        assert_eq!(clock(75.9, ':'), "01:15");
        assert_eq!(clock(3725.0, '-'), "1-02-05");
        assert!(Clip::new(5.0, 5.2, false).is_none());
        assert!(Clip::new(-1.0, 5.0, false).is_none());
        let c = Clip::new(80.0, 225.0, false).unwrap();
        assert_eq!(DownloadRecord::clip_name("Trip [1080p].webm", &c), "Trip [1080p] [01-20 → 03-45].mp4");
        assert_eq!(DownloadRecord::clip_name("Song.mp3", &c), "Song [01-20 → 03-45].mp3");
    }
}
