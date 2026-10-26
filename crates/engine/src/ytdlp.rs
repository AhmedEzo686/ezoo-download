//! Video links from any website through yt-dlp.
//!
//! yt-dlp knows how to find the real video files on more than 1800 sites
//! (YouTube, Facebook, Instagram, TikTok, X/Twitter, Vimeo, Dailymotion...)
//! and also has a generic extractor for ordinary pages with a video.
//! We only use it to *find* the links (`yt-dlp -J`); the actual download
//! still runs through our multi-connection engine.

use crate::model::RequestHeaders;
use crate::mux::{find_tool, hide_window};
use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

/// One choice in the "Download this video" menu.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct YtOption {
    /// Stable id of this choice ("1080p60|MP4"), used to get fresh links later
    pub key: String,
    pub label: String,
    pub sub: String,
    pub quality: String,
    pub video_url: String,
    pub audio_url: Option<String>,
    pub ext: String,
    pub size: u64,
    /// headers these links need (mostly the User-Agent and Referer)
    pub headers: Vec<(String, String)>,
    /// plain words for the resolution: "SD", "HD", "Full HD", "2K", "4K", "8K"
    pub tag: String,
    /// "H.264", "VP9", "AV1" ("" = not known / audio)
    pub codec: String,
    /// length of the video in seconds (0 = unknown)
    pub duration: u64,
    /// bitrate in kbit/s (0 = unknown)
    pub kbps: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YtInfo {
    pub title: String,
    /// e.g. "YouTube", "Instagram", "TikTok"
    pub site: String,
    pub options: Vec<YtOption>,
}

/// A cookie as the browser extension sends it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCookie {
    pub domain: String,
    pub name: String,
    pub value: String,
    #[serde(default = "root_path")]
    pub path: String,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub expiration_date: Option<f64>,
}

fn root_path() -> String {
    "/".into()
}

#[derive(Debug, Clone)]
struct Fmt {
    url: String,
    ext: String,
    vcodec: String,
    acodec: String,
    height: u64,
    fps: u64,
    size: u64,
    abr: f64,
    tbr: f64,
    note: String,
    hls: bool,
    headers: Vec<(String, String)>,
    /// sound track ranking: the video's own language first, translated
    /// ("dubbed", often made automatically) tracks last
    audio_rank: i64,
}

impl Fmt {
    /// "unknown" codecs (common outside YouTube) count as present
    fn has_video(&self) -> bool {
        self.vcodec != "none"
    }
    fn has_audio(&self) -> bool {
        self.acodec != "none"
    }
    /// lower is better: H.264 plays everywhere, then VP9, then AV1
    fn codec_rank(&self) -> u8 {
        let c = self.vcodec.to_ascii_lowercase();
        if c.starts_with("avc") || c.starts_with("h264") || c == "unknown" {
            0
        } else if c.starts_with("vp9") || c.starts_with("vp09") {
            1
        } else {
            2
        }
    }
    fn codec_name(&self) -> &'static str {
        let c = self.vcodec.to_ascii_lowercase();
        if c.starts_with("av01") {
            "AV1"
        } else if c.starts_with("vp9") || c.starts_with("vp09") {
            "VP9"
        } else if c.starts_with("avc") || c.starts_with("h264") {
            "H.264"
        } else if c.starts_with("hev") || c.starts_with("hvc") || c.starts_with("h265") {
            "H.265"
        } else {
            ""
        }
    }
    fn codec_tag(&self) -> &'static str {
        if self.vcodec.to_ascii_lowercase().starts_with("av01") {
            " AV1"
        } else {
            ""
        }
    }
}

fn as_u64(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0).round() as u64))
}

fn codec(v: &Value) -> String {
    match v.as_str() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => "unknown".into(),
    }
}

fn headers_of(f: &Value) -> Vec<(String, String)> {
    f["http_headers"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_one(f: &Value) -> Option<Fmt> {
    let protocol = f["protocol"].as_str().unwrap_or("https");
    let hls = protocol.starts_with("m3u8");
    if !(protocol == "https" || protocol == "http" || hls) {
        return None; // dash manifests, storyboards, rtmp, live fragments
    }
    if f["has_drm"].as_bool() == Some(true) {
        return None;
    }
    let ext = f["ext"].as_str().unwrap_or("mp4").to_ascii_lowercase();
    if ext == "mhtml" {
        return None;
    }
    Some(Fmt {
        url: f["url"].as_str()?.to_string(),
        ext,
        vcodec: codec(&f["vcodec"]),
        acodec: codec(&f["acodec"]),
        height: as_u64(&f["height"]).unwrap_or(0),
        fps: as_u64(&f["fps"]).unwrap_or(0),
        size: as_u64(&f["filesize"]).or_else(|| as_u64(&f["filesize_approx"])).unwrap_or(0),
        abr: f["abr"].as_f64().or_else(|| f["tbr"].as_f64()).unwrap_or(0.0),
        tbr: f["tbr"].as_f64().or_else(|| f["vbr"].as_f64()).unwrap_or(0.0),
        note: f["format_note"].as_str().unwrap_or("").to_string(),
        hls,
        headers: headers_of(f),
        audio_rank: audio_rank(f),
    })
}

/// Videos can carry several sound tracks in different languages; yt-dlp marks
/// the original one (language_preference, "original" / "default" in the
/// note) and translated ones ("dubbed"). Higher = better.
fn audio_rank(f: &Value) -> i64 {
    let note = format!(
        "{} {}",
        f["format_note"].as_str().unwrap_or(""),
        f["format_id"].as_str().unwrap_or("")
    )
    .to_ascii_lowercase();
    let mut rank = f["language_preference"].as_i64().unwrap_or(0);
    if note.contains("original") {
        rank += 100;
    } else if note.contains("default") {
        rank += 50;
    }
    if note.contains("dubbed") || note.contains("descriptive") {
        rank -= 100;
    }
    rank
}

fn parse_formats(info: &Value) -> Vec<Fmt> {
    match info["formats"].as_array() {
        Some(list) => list.iter().filter_map(parse_one).collect(),
        // some sites give a single file with no "formats" list
        None => parse_one(info).into_iter().collect(),
    }
}

/// Posts with several videos (playlists, carousels): use the first video.
fn main_entry(info: &Value) -> &Value {
    if info["_type"].as_str() == Some("playlist") {
        if let Some(entry) = info["entries"]
            .as_array()
            .and_then(|e| e.iter().find(|x| x["formats"].is_array() || x["url"].is_string()))
        {
            return entry;
        }
    }
    info
}

fn fmt_size(n: u64) -> String {
    if n == 0 {
        return "size unknown".into();
    }
    crate::model::fmt_size(n)
}

fn quality_of(f: &Fmt) -> String {
    let mut q = if f.height > 0 {
        format!("{}p", f.height)
    } else if !f.note.is_empty() && f.note.len() <= 12 {
        f.note.to_ascii_uppercase()
    } else {
        "Video".into()
    };
    if f.fps > 30 {
        q.push_str(&f.fps.to_string());
    }
    q
}

/// The "MP3" choice: the sound is downloaded, then turned into an MP3 file.
pub const MP3_KEY: &str = "audio|MP3";
/// bitrate of the MP3 files we make
pub const MP3_KBPS: u64 = 192;

/// Plain words for a resolution, so the choice is easy to understand.
pub fn quality_tag(height: u64) -> &'static str {
    match height {
        0 => "",
        h if h >= 4320 => "8K",
        h if h >= 2160 => "4K",
        h if h >= 1440 => "2K",
        h if h >= 1080 => "Full HD",
        h if h >= 720 => "HD",
        _ => "SD",
    }
}

fn with_tag(q: &str, height: u64) -> String {
    match quality_tag(height) {
        "" => q.to_string(),
        t => format!("{q} {t}"),
    }
}

/// Turn yt-dlp's JSON into download choices: every resolution paired with the
/// best matching sound, plus "audio only".
pub fn build_options(full: &Value) -> YtInfo {
    let info = main_entry(full);
    let title = info["title"]
        .as_str()
        .or_else(|| full["title"].as_str())
        .unwrap_or("video")
        .to_string();
    let site = info["extractor_key"]
        .as_str()
        .or_else(|| full["extractor_key"].as_str())
        .unwrap_or("")
        .to_string();
    let formats = parse_formats(info);
    let duration = as_u64(&info["duration"]).unwrap_or(0);

    let mut audios: Vec<&Fmt> = formats
        .iter()
        .filter(|f| f.has_audio() && !f.has_video() && !f.hls)
        .collect();
    // the original language first, then the best quality
    audios.sort_by(|a, b| {
        b.audio_rank
            .cmp(&a.audio_rank)
            .then(b.abr.partial_cmp(&a.abr).unwrap_or(std::cmp::Ordering::Equal))
    });
    let best_audio = |container: &str| -> Option<&Fmt> {
        audios
            .iter()
            .copied()
            .find(|a| if container == "webm" { a.ext == "webm" } else { a.ext == "m4a" || a.ext == "mp4" })
    };

    let mut videos: Vec<&Fmt> = formats.iter().filter(|f| f.has_video()).collect();
    videos.sort_by(|a, b| {
        b.height
            .cmp(&a.height)
            .then(b.fps.cmp(&a.fps))
            .then(a.hls.cmp(&b.hls)) // direct files before streams
            .then(a.codec_rank().cmp(&b.codec_rank()))
            .then(b.size.cmp(&a.size))
    });

    let mut options = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for v in videos {
        let q = quality_of(v);
        let container = if v.ext == "webm" { "webm" } else { "mp4" };
        if v.has_audio() {
            // video and sound in one file (most sites besides YouTube)
            let key = format!("{q}|{}+A", container.to_uppercase());
            if !seen.insert(key.clone()) {
                continue;
            }
            let ext = if v.hls { "ts" } else { container };
            options.push(YtOption {
                label: format!("{} · {}", with_tag(&q, v.height), ext.to_uppercase()),
                sub: fmt_size(v.size),
                key,
                quality: q,
                video_url: v.url.clone(),
                audio_url: None,
                ext: ext.into(),
                size: v.size,
                headers: v.headers.clone(),
                tag: quality_tag(v.height).into(),
                codec: v.codec_name().into(),
                duration,
                kbps: v.tbr.round() as u64,
            });
            continue;
        }
        if v.hls {
            continue; // video-only stream: we can't add sound to it
        }
        let key = format!("{q}|{}", container.to_uppercase());
        if !seen.insert(key.clone()) {
            continue; // already have this resolution in this container (better codec)
        }
        let (audio, ext) = match best_audio(container) {
            Some(a) => (a, container.to_string()),
            None => match audios.first() {
                Some(a) => (*a, "mkv".to_string()),
                None => continue,
            },
        };
        let size = if v.size > 0 && audio.size > 0 { v.size + audio.size } else { 0 };
        options.push(YtOption {
            label: format!("{} · {}{}", with_tag(&q, v.height), ext.to_uppercase(), v.codec_tag()),
            sub: fmt_size(size),
            key,
            quality: q,
            video_url: v.url.clone(),
            audio_url: Some(audio.url.clone()),
            ext,
            size,
            headers: v.headers.clone(),
            tag: quality_tag(v.height).into(),
            codec: v.codec_name().into(),
            duration,
            kbps: (v.tbr + audio.abr).round() as u64,
        });
    }
    if let Some(a) = best_audio("mp4").or_else(|| audios.first().copied()) {
        let ext = if a.ext == "webm" { "webm" } else if a.ext == "mp3" { "mp3" } else { "m4a" };
        let kbps = a.abr.round() as u64;
        options.push(YtOption {
            key: format!("audio|{}", ext.to_uppercase()),
            label: if kbps > 0 {
                format!("Audio only · {} {kbps}k", ext.to_uppercase())
            } else {
                format!("Audio only · {}", ext.to_uppercase())
            },
            sub: fmt_size(a.size),
            quality: "audio".into(),
            video_url: a.url.clone(),
            audio_url: None,
            ext: ext.into(),
            size: a.size,
            headers: a.headers.clone(),
            tag: String::new(),
            codec: String::new(),
            duration,
            kbps,
        });
        if ext != "mp3" {
            // the same sound, turned into an MP3 file after the download
            // (plays on every phone, car radio and old player)
            let mp3_size = if duration > 0 { duration * MP3_KBPS * 1000 / 8 } else { 0 };
            options.push(YtOption {
                key: MP3_KEY.into(),
                label: format!("Audio · MP3 {MP3_KBPS}k"),
                sub: fmt_size(mp3_size),
                quality: "mp3".into(),
                video_url: a.url.clone(),
                audio_url: None,
                ext: "mp3".into(),
                size: mp3_size,
                headers: a.headers.clone(),
                tag: String::new(),
                codec: String::new(),
                duration,
                kbps: MP3_KBPS,
            });
        }
    }
    YtInfo { title, site, options }
}

/// Use the headers yt-dlp asked for when downloading the links.
pub fn apply_headers(target: &mut RequestHeaders, headers: &[(String, String)]) {
    for (k, v) in headers {
        let lower = k.to_ascii_lowercase();
        if lower == "user-agent" {
            target.user_agent = Some(v.clone());
        } else if lower == "referer" {
            target.referer = Some(v.clone());
        } else if lower == "accept" || lower == "accept-encoding" || lower == "cookie" || lower == "range" {
            // we set these ourselves
        } else {
            target.extra.retain(|(ek, _)| !ek.eq_ignore_ascii_case(k));
            target.extra.push((k.clone(), v.clone()));
        }
    }
}

/// Netscape cookie file (what yt-dlp reads) from the browser's cookies.
pub fn netscape_cookies(cookies: &[BrowserCookie]) -> String {
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for c in cookies {
        if c.name.is_empty() || c.domain.is_empty() || c.name.contains(['\t', '\n']) || c.value.contains(['\t', '\n']) {
            continue;
        }
        let sub = if c.domain.starts_with('.') { "TRUE" } else { "FALSE" };
        let expires = c.expiration_date.map(|e| e.max(0.0) as u64).unwrap_or(2_147_483_647);
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            c.domain,
            sub,
            if c.path.is_empty() { "/" } else { &c.path },
            if c.secure { "TRUE" } else { "FALSE" },
            expires,
            c.name,
            c.value
        ));
    }
    out
}

/// The arguments for looking at one page. `--ignore-config`: settings files
/// of yt-dlp on the computer (which could add `--exec` and the like) are not
/// read; `--`: the address can never be taken for an option.
fn resolve_args(page_url: &str, cookie_file: Option<&Path>) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = ["--ignore-config", "-J", "--no-playlist", "--no-warnings", "--no-progress", "--socket-timeout", "20"]
        .iter()
        .map(Into::into)
        .collect();
    if let Some(c) = cookie_file {
        a.push("--cookies".into());
        a.push(c.into());
    }
    a.push("--".into());
    a.push(page_url.into());
    a
}

fn playlist_args(url: &str, cookie_file: Option<&Path>) -> Vec<std::ffi::OsString> {
    let mut a: Vec<std::ffi::OsString> = ["--ignore-config", "-J", "--flat-playlist", "--yes-playlist", "--no-warnings", "--socket-timeout", "20"]
        .iter()
        .map(Into::into)
        .collect();
    if let Some(c) = cookie_file {
        a.push("--cookies".into());
        a.push(c.into());
    }
    a.push("--".into());
    a.push(url.into());
    a
}

/// Owns a cookie file until the helper finishes, including unwinding on panic.
struct CookieFile {
    path: std::path::PathBuf,
    file: Option<std::fs::File>,
}

impl CookieFile {
    fn write(mut self, contents: &[u8]) -> std::io::Result<Self> {
        use std::io::Write;
        self.file.as_mut().expect("new cookie file").write_all(contents)?;
        drop(self.file.take());
        Ok(self)
    }
}

impl Drop for CookieFile {
    fn drop(&mut self) {
        // Windows cannot remove an open file. Close it before cleanup, also
        // when writing failed or the helper panicked.
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A new, private file for the browser's cookies (never one that exists).
fn cookie_file(cookies: &[BrowserCookie]) -> anyhow::Result<CookieFile> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let contents = netscape_cookies(cookies);
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    for _ in 0..5 {
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("ezoo-cookies-{}-{nanos:x}-{n}.txt", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok(CookieFile { path, file: Some(file) }.write(contents.as_bytes())?),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    bail!("could not make a cookie file")
}

fn run_ytdlp(tool: &Path, page_url: &str, cookie_file: Option<&Path>) -> anyhow::Result<Value> {
    let mut cmd = Command::new(tool);
    cmd.args(resolve_args(page_url, cookie_file));
    hide_window(&mut cmd);
    let out = cmd.output().context("could not start yt-dlp")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .rev()
            .find(|l| l.contains("ERROR"))
            .or_else(|| err.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or("unknown error")
            .trim()
            .trim_start_matches("ERROR:")
            .trim()
            .to_string();
        bail!("{line}");
    }
    serde_json::from_slice(&out.stdout).context("yt-dlp returned something unexpected")
}

fn needs_login(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    ["sign in", "log in", "login", "cookies", "age", "bot", "private", "registered users", "rate-limit", "empty media"]
        .iter()
        .any(|k| m.contains(k))
}

/// The site part of an address that cookies belong to ("www.youtube.com",
/// "m.youtube.com" and "youtu.be" are all "youtube.com").
fn site_of(url: &str) -> String {
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default();
    if host == "youtu.be" || host.ends_with(".youtu.be") {
        return "youtube.com".into();
    }
    let parts: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    // co.uk, com.eg, ...: keep three labels
    let keep = if parts.len() >= 3 && parts[parts.len() - 2].len() <= 3 && parts[parts.len() - 1].len() == 2 { 3 } else { 2 };
    parts[parts.len().saturating_sub(keep)..].join(".")
}

/// The browser's cookies of each site, kept in memory while the program
/// runs (a private temporary file is needed for cookie-assisted lookups): a quality picked later, the next
/// video of a playlist or an address typed in "Add URL" can then also open
/// videos that need a login or an age check.
static JAR: std::sync::Mutex<Vec<(String, Vec<BrowserCookie>)>> = std::sync::Mutex::new(Vec::new());

/// Keep the browser's cookies of a site for later (replaces older ones).
pub fn remember_cookies(page_url: &str, cookies: &[BrowserCookie]) {
    if cookies.is_empty() {
        return;
    }
    let site = site_of(page_url);
    if site.is_empty() {
        return;
    }
    let mut jar = JAR.lock().unwrap_or_else(|p| p.into_inner());
    jar.retain(|(s, _)| *s != site);
    jar.push((site, cookies.to_vec()));
    if jar.len() > 20 {
        jar.remove(0);
    }
}

/// The cookies the browser gave for this site, if any.
pub fn remembered_cookies(page_url: &str) -> Option<Vec<BrowserCookie>> {
    let site = site_of(page_url);
    let jar = JAR.lock().unwrap_or_else(|p| p.into_inner());
    jar.iter().find(|(s, _)| *s == site).map(|(_, c)| c.clone())
}

/// Clearly a login or age wall (not just any error with "age" in it).
fn login_wall(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    ["sign in", "log in", "login", "age-restricted", "age restricted", "confirm your age", "inappropriate for some users", "registered users", "private video", "cookies"]
        .iter()
        .any(|k| m.contains(k))
}

/// The error shown when a video needs a login or an age check and no
/// cookies of the browser help.
pub const LOGIN_NEEDED: &str = "this video needs a login or an age check: open it once in the browser where you are signed in (with the Ezoo Download extension), then try again";

#[derive(Clone, Copy)]
enum CookieAccess {
    Remembered,
    SuppliedOnly,
}

fn with_cookies<T>(
    page_url: &str,
    cookies: Option<&[BrowserCookie]>,
    access: CookieAccess,
    run: impl Fn(Option<&Path>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    if matches!(access, CookieAccess::Remembered) {
        if let Some(list) = cookies {
            remember_cookies(page_url, list);
        }
    }
    match run(None) {
        Ok(v) => Ok(v),
        Err(e) if needs_login(&e.to_string()) => {
            let list = cookies.map(<[BrowserCookie]>::to_vec).or_else(|| {
                matches!(access, CookieAccess::Remembered).then(|| remembered_cookies(page_url)).flatten()
            });
            let Some(list) = list.filter(|c| !c.is_empty()) else {
                return Err(if login_wall(&e.to_string()) { e.context(LOGIN_NEEDED) } else { e });
            };
            let file = cookie_file(&list)?;
            let r = run(Some(&file.path));
            r.map_err(|e2| if login_wall(&e2.to_string()) { e2.context(LOGIN_NEEDED) } else { e2 })
        }
        Err(e) => Err(e),
    }
}

/// Find the download choices of a page with a video. Blocking (takes a few
/// seconds). Tries without cookies first, and with the browser's cookies for
/// that site only when the site asks for a login (Instagram, Facebook,
/// age-limited YouTube videos, ...). Cookies the browser gave earlier for
/// the same site are used when none come with this call.
pub fn resolve(page_url: &str, cookies: Option<&[BrowserCookie]>) -> anyhow::Result<YtInfo> {
    resolve_using(page_url, cookies, CookieAccess::Remembered)
}

/// A loopback API caller may use only cookies supplied in that request.
/// Successful explicit cookies are remembered for later desktop-initiated
/// playlist/renewal operations, but never borrowed by another API request.
pub fn resolve_for_browser(page_url: &str, cookies: &[BrowserCookie]) -> anyhow::Result<YtInfo> {
    let info = resolve_using(page_url, (!cookies.is_empty()).then_some(cookies), CookieAccess::SuppliedOnly)?;
    remember_cookies(page_url, cookies);
    Ok(info)
}

fn resolve_using(page_url: &str, cookies: Option<&[BrowserCookie]>, access: CookieAccess) -> anyhow::Result<YtInfo> {
    let tool = find_tool("yt-dlp")
        .ok_or_else(|| anyhow!(crate::tools::missing_tool_message("yt-dlp")))?;
    let cookies = cookies.filter(|c| !c.is_empty());
    let json = with_cookies(page_url, cookies, access, |file| run_ytdlp(&tool, page_url, file))?;
    let info = build_options(&json);
    if info.options.is_empty() {
        bail!("no downloadable video was found on this page");
    }
    Ok(info)
}

/// Fresh links for a choice made earlier (links expire after a few hours).
pub fn resolve_key(page_url: &str, key: &str) -> anyhow::Result<YtOption> {
    resolve(page_url, None)?
        .options
        .into_iter()
        .find(|o| o.key == key)
        .ok_or_else(|| anyhow!("this quality is not offered any more"))
}

/// One video of a playlist.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistEntry {
    pub title: String,
    pub url: String,
    /// seconds (0 = unknown)
    pub duration: u64,
}

#[derive(Debug, Clone)]
pub struct PlaylistInfo {
    pub title: String,
    pub entries: Vec<PlaylistEntry>,
}

/// The videos of a playlist (or channel page), without looking at each
/// video yet (`--flat-playlist`, fast). Blocking.
pub fn playlist(url: &str) -> anyhow::Result<PlaylistInfo> {
    let tool = find_tool("yt-dlp").ok_or_else(|| anyhow!(crate::tools::missing_tool_message("yt-dlp")))?;
    let json = with_cookies(url, None, CookieAccess::Remembered, |file| {
        let mut cmd = Command::new(&tool);
        cmd.args(playlist_args(url, file));
        hide_window(&mut cmd);
        let out = cmd.output().context("could not start yt-dlp")?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
            bail!("{}", line.trim().trim_start_matches("ERROR:").trim());
        }
        serde_json::from_slice::<Value>(&out.stdout).context("yt-dlp returned something unexpected")
    })?;
    Ok(parse_playlist(&json))
}

pub fn parse_playlist(json: &Value) -> PlaylistInfo {
    let title = json["title"].as_str().unwrap_or("Playlist").to_string();
    let mut entries = Vec::new();
    let list = json["entries"].as_array().cloned().unwrap_or_default();
    if list.is_empty() && json["_type"].as_str() != Some("playlist") {
        // a single video
        if let Some(u) = json["webpage_url"].as_str().or_else(|| json["original_url"].as_str()) {
            entries.push(PlaylistEntry {
                title: title.clone(),
                url: u.to_string(),
                duration: as_u64(&json["duration"]).unwrap_or(0),
            });
        }
    }
    for e in list {
        let url = e["url"]
            .as_str()
            .filter(|u| u.starts_with("http"))
            .map(String::from)
            .or_else(|| e["webpage_url"].as_str().map(String::from))
            .or_else(|| e["id"].as_str().map(|id| format!("https://www.youtube.com/watch?v={id}")));
        let Some(url) = url else { continue };
        let t = e["title"].as_str().unwrap_or("video");
        // deleted / private videos
        if t == "[Deleted video]" || t == "[Private video]" {
            continue;
        }
        entries.push(PlaylistEntry { title: t.to_string(), url, duration: as_u64(&e["duration"]).unwrap_or(0) });
    }
    PlaylistInfo { title, entries }
}

/// Height of a choice ("1080p60" -> 1080; audio / unknown -> 0).
fn height_of(o: &YtOption) -> u64 {
    o.quality.split('p').next().and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// Pick a choice by a wanted quality: "best", "audio" or a height like
/// "720" (the best one not above it, MP4 first; the lowest if all are higher).
pub fn pick_option(options: &[YtOption], wanted: &str) -> Option<YtOption> {
    if wanted == "mp3" {
        return options.iter().find(|o| o.key == MP3_KEY).cloned();
    }
    if wanted == "audio" {
        return options.iter().find(|o| o.key.starts_with("audio|") && o.key != MP3_KEY).cloned();
    }
    let videos: Vec<&YtOption> = options.iter().filter(|o| !o.key.starts_with("audio|")).collect();
    if videos.is_empty() {
        return options.first().cloned();
    }
    let limit: u64 = wanted.parse().unwrap_or(u64::MAX);
    let fits: Vec<&YtOption> = videos.iter().copied().filter(|o| height_of(o) <= limit).collect();
    let pool = if fits.is_empty() {
        let low = videos.iter().map(|o| height_of(o)).min().unwrap_or(0);
        videos.iter().copied().filter(|o| height_of(o) == low).collect::<Vec<_>>()
    } else {
        let top = fits.iter().map(|o| height_of(o)).max().unwrap_or(0);
        fits.into_iter().filter(|o| height_of(o) == top).collect()
    };
    pool.iter().find(|o| o.ext == "mp4").or(pool.first()).map(|o| (*o).clone())
}

/// Update yt-dlp right now ("Help > Update video downloader"). Blocking.
pub fn update_now() -> anyhow::Result<String> {
    let tool = find_tool("yt-dlp").ok_or_else(|| anyhow!(crate::tools::missing_tool_message("yt-dlp")))?;
    if cfg!(target_os = "macos") {
        return Ok("On macOS, update the video downloader with `brew upgrade yt-dlp`, then try again.".into());
    }
    let mut cmd = Command::new(&tool);
    cmd.args(["--ignore-config", "-U"]);
    hide_window(&mut cmd);
    let out = cmd.output().context("could not start yt-dlp")?;
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("done").trim().to_string();
    if out.status.success() {
        Ok(last)
    } else {
        bail!("{last}")
    }
}

/// yt-dlp needs updates when sites change things. Runs `yt-dlp -U` at most
/// once a day, for the copy next to the app. Blocking; call it from a
/// background thread.
pub fn update_if_due(data_dir: &Path) {
    if cfg!(target_os = "macos") {
        return;
    }
    let stamp = data_dir.join("yt-dlp-updated");
    let due = std::fs::metadata(&stamp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|age| age > Duration::from_secs(24 * 3600))
        .unwrap_or(true);
    if !due {
        return;
    }
    let Some(tool) = find_tool("yt-dlp") else { return };
    if !tool.is_absolute() {
        return; // installed elsewhere (e.g. winget): leave updates to that
    }
    let mut cmd = Command::new(&tool);
    cmd.args(["--ignore-config", "-U"]);
    hide_window(&mut cmd);
    if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
        let _ = std::fs::write(&stamp, b"ok");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn youtube_sample() -> Value {
        let h = json!({"User-Agent": "Mozilla/5.0 test", "Accept": "*/*", "Sec-Fetch-Mode": "navigate"});
        json!({
            "title": "Cool video",
            "extractor_key": "Youtube",
            "formats": [
                {"format_id": "sb0", "protocol": "mhtml", "ext": "mhtml", "vcodec": "none", "acodec": "none", "url": "https://i.ytimg.com/sb"},
                {"format_id": "139", "protocol": "https", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.5", "abr": 48.8, "filesize": 1000, "url": "https://gv/139", "http_headers": h},
                {"format_id": "140", "protocol": "https", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 129.5, "filesize": 4000, "url": "https://gv/140", "http_headers": h},
                {"format_id": "251", "protocol": "https", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 135.0, "filesize": 4200, "url": "https://gv/251", "http_headers": h},
                {"format_id": "18", "protocol": "https", "ext": "mp4", "vcodec": "avc1.42001E", "acodec": "mp4a.40.2", "height": 360, "fps": 25, "filesize_approx": 9000.4, "url": "https://gv/18", "http_headers": h},
                {"format_id": "136", "protocol": "https", "ext": "mp4", "vcodec": "avc1.4d401f", "acodec": "none", "height": 720, "fps": 25, "filesize": 50000, "url": "https://gv/136", "http_headers": h},
                {"format_id": "399", "protocol": "https", "ext": "mp4", "vcodec": "av01.0.08M.08", "acodec": "none", "height": 1080, "fps": 25, "filesize": 70000, "url": "https://gv/399", "http_headers": h},
                {"format_id": "137", "protocol": "https", "ext": "mp4", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "fps": 25, "filesize": 90000, "url": "https://gv/137", "http_headers": h},
                {"format_id": "248", "protocol": "https", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 1080, "fps": 25, "filesize": 80000, "url": "https://gv/248", "http_headers": h},
                {"format_id": "303", "protocol": "https", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 1080, "fps": 60, "filesize": 120000, "url": "https://gv/303", "http_headers": h},
                {"format_id": "drm", "protocol": "https", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "height": 2160, "has_drm": true, "url": "https://gv/drm"}
            ]
        })
    }

    #[test]
    fn original_sound_track_wins_over_dubbed() {
        let info = serde_json::json!({
            "title": "t", "extractor_key": "Youtube",
            "formats": [
                {"format_id": "140-0", "protocol": "https", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 130.0,
                 "language": "en", "language_preference": -1, "format_note": "English (United States) - dubbed-auto, medium", "url": "https://gv/en"},
                {"format_id": "140-1", "protocol": "https", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 129.0,
                 "language": "ar", "language_preference": 10, "format_note": "Arabic original (default), medium", "url": "https://gv/ar"},
                {"format_id": "137", "protocol": "https", "ext": "mp4", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "url": "https://gv/137"}
            ]
        });
        let r = build_options(&info);
        let best = r.options.iter().find(|o| o.key.starts_with("1080p")).unwrap();
        assert_eq!(best.audio_url.as_deref(), Some("https://gv/ar"));
        let audio = r.options.iter().find(|o| o.key.starts_with("audio|")).unwrap();
        assert_eq!(audio.video_url, "https://gv/ar");
    }

    #[test]
    fn youtube_choices() {
        let info = build_options(&youtube_sample());
        assert_eq!(info.title, "Cool video");
        assert_eq!(info.site, "Youtube");
        let keys: Vec<&str> = info.options.iter().map(|o| o.key.as_str()).collect();
        assert_eq!(keys, ["1080p60|WEBM", "1080p|MP4", "1080p|WEBM", "720p|MP4", "360p|MP4+A", "audio|M4A", "audio|MP3"]);
        let mp4 = &info.options[1];
        assert_eq!(mp4.video_url, "https://gv/137");
        assert_eq!(mp4.audio_url.as_deref(), Some("https://gv/140"));
        assert_eq!(mp4.size, 94000);
        assert_eq!(info.options[0].audio_url.as_deref(), Some("https://gv/251"));
        assert_eq!(info.options[5].label, "Audio only · M4A 130k");
        assert_eq!(info.options[6].ext, "mp3");
        assert_eq!(info.options[6].video_url, info.options[5].video_url);
        // the resolution in plain words
        assert_eq!(mp4.label, "1080p Full HD · MP4");
        assert_eq!(mp4.tag, "Full HD");
        assert_eq!(info.options[3].tag, "HD");
        assert_eq!(info.options[4].tag, "SD");
        assert_eq!(pick_option(&info.options, "mp3").unwrap().key, MP3_KEY);
        assert_eq!(pick_option(&info.options, "audio").unwrap().key, "audio|M4A");
    }

    #[test]
    fn plain_words_for_resolutions() {
        assert_eq!(quality_tag(4320), "8K");
        assert_eq!(quality_tag(2160), "4K");
        assert_eq!(quality_tag(1440), "2K");
        assert_eq!(quality_tag(1080), "Full HD");
        assert_eq!(quality_tag(720), "HD");
        assert_eq!(quality_tag(480), "SD");
        assert_eq!(quality_tag(0), "");
    }

    #[test]
    fn other_sites_without_codecs() {
        // TikTok / Instagram style: codecs not reported, video has sound
        let j = json!({
            "title": "Funny clip",
            "extractor_key": "TikTok",
            "formats": [
                {"format_id": "h264_540p", "protocol": "https", "ext": "mp4", "height": 1024, "width": 576, "filesize": 3000, "url": "https://v16/a.mp4", "http_headers": {"Referer": "https://www.tiktok.com/"}},
                {"format_id": "download", "protocol": "https", "ext": "mp4", "format_note": "watermarked", "url": "https://v16/b.mp4"},
                {"format_id": "hls-720", "protocol": "m3u8_native", "ext": "mp4", "height": 720, "vcodec": "avc1", "acodec": "mp4a", "url": "https://cdn/x.m3u8"}
            ]
        });
        let info = build_options(&j);
        let keys: Vec<&str> = info.options.iter().map(|o| o.key.as_str()).collect();
        assert_eq!(keys, ["1024p|MP4+A", "720p|MP4+A", "WATERMARKED|MP4+A"]);
        assert_eq!(info.options[0].audio_url, None);
        assert_eq!(info.options[1].ext, "ts");
        let mut h = RequestHeaders::default();
        apply_headers(&mut h, &info.options[0].headers);
        assert_eq!(h.referer.as_deref(), Some("https://www.tiktok.com/"));
    }

    #[test]
    fn single_file_and_playlist() {
        let single = json!({"title": "Clip", "url": "https://x/v.mp4", "ext": "mp4", "protocol": "https"});
        assert_eq!(build_options(&single).options.len(), 1);
        let post = json!({"_type": "playlist", "title": "Post", "entries": [
            {"title": "Video 1", "formats": [{"url": "https://x/1.mp4", "ext": "mp4", "protocol": "https", "height": 640}]}
        ]});
        let info = build_options(&post);
        assert_eq!(info.title, "Video 1");
        assert_eq!(info.options[0].video_url, "https://x/1.mp4");
    }

    #[test]
    fn playlists_and_quality_choice() {
        let j = json!({"_type": "playlist", "title": "Course", "entries": [
            {"id": "aaa", "title": "Lesson 1", "url": "https://www.youtube.com/watch?v=aaa", "duration": 61.0},
            {"id": "bbb", "title": "[Private video]", "url": "https://www.youtube.com/watch?v=bbb"},
            {"id": "ccc", "title": "Lesson 3"}
        ]});
        let p = parse_playlist(&j);
        assert_eq!(p.title, "Course");
        assert_eq!(p.entries.len(), 2);
        assert_eq!(p.entries[0].duration, 61);
        assert_eq!(p.entries[1].url, "https://www.youtube.com/watch?v=ccc");

        let opts = build_options(&youtube_sample()).options;
        assert_eq!(pick_option(&opts, "best").unwrap().key, "1080p|MP4");
        assert_eq!(pick_option(&opts, "1080").unwrap().key, "1080p|MP4");
        assert_eq!(pick_option(&opts, "720").unwrap().key, "720p|MP4");
        assert_eq!(pick_option(&opts, "480").unwrap().key, "360p|MP4+A");
        assert_eq!(pick_option(&opts, "240").unwrap().key, "360p|MP4+A");
        assert_eq!(pick_option(&opts, "audio").unwrap().key, "audio|M4A");
    }

    #[test]
    fn headers_are_applied() {
        let mut h = RequestHeaders::default();
        apply_headers(&mut h, &build_options(&youtube_sample()).options[0].headers);
        assert_eq!(h.user_agent.as_deref(), Some("Mozilla/5.0 test"));
        assert!(h.extra.iter().any(|(k, _)| k == "Sec-Fetch-Mode"));
        assert!(!h.extra.iter().any(|(k, _)| k == "Accept"));
    }

    #[test]
    fn yt_dlp_settings_files_and_options_are_never_used() {
        let a: Vec<String> = resolve_args("-x --exec calc", Some(Path::new("c.txt"))).iter().map(|x| x.to_string_lossy().into_owned()).collect();
        assert_eq!(a[0], "--ignore-config");
        // the address comes after "--": yt-dlp can't read it as an option
        assert_eq!(&a[a.len() - 2..], ["--", "-x --exec calc"]);
        let p: Vec<String> = playlist_args("-o x", None).iter().map(|x| x.to_string_lossy().into_owned()).collect();
        assert_eq!(p[0], "--ignore-config");
        assert_eq!(&p[p.len() - 2..], ["--", "-o x"]);
    }

    #[test]
    fn cookies_are_kept_per_site() {
        assert_eq!(site_of("https://www.youtube.com/watch?v=x"), "youtube.com");
        assert_eq!(site_of("https://youtu.be/x"), "youtube.com");
        assert_eq!(site_of("https://m.youtube.com/watch?v=x"), "youtube.com");
        assert_eq!(site_of("https://www.bbc.co.uk/x"), "bbc.co.uk");
        let c = BrowserCookie {
            domain: ".youtube.com".into(),
            name: "SID".into(),
            value: "v".into(),
            path: "/".into(),
            secure: true,
            expiration_date: None,
        };
        remember_cookies("https://www.youtube.com/watch?v=a", &[c]);
        assert_eq!(remembered_cookies("https://youtu.be/b").unwrap()[0].name, "SID");
        assert!(remembered_cookies("https://vimeo.com/1").is_none());
        assert!(login_wall("Sign in to confirm your age. This video may be inappropriate for some users."));
        assert!(!login_wall("Unsupported URL: https://example.com/page"));
    }

    #[test]
    fn cookie_files_are_new_every_time() {
        let list = vec![BrowserCookie {
            domain: ".example.com".into(),
            name: "a".into(),
            value: "b".into(),
            path: "/".into(),
            secure: true,
            expiration_date: None,
        }];
        let (a, b) = (cookie_file(&list).unwrap(), cookie_file(&list).unwrap());
        assert_ne!(a.path, b.path);
        assert!(std::fs::read_to_string(&a.path).unwrap().contains("example.com"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&a.path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let path = a.path.clone();
        drop(a);
        assert!(!path.exists());
    }

    fn synthetic_cookie(value: &str) -> BrowserCookie {
        BrowserCookie {
            domain: ".session-isolation.invalid".into(), name: "session".into(), value: value.into(),
            path: "/".into(), secure: true, expiration_date: None,
        }
    }

    #[test]
    fn browser_resolution_cannot_borrow_remembered_cookies() {
        let page = "https://session-isolation.invalid/private-video";
        remember_cookies(page, &[synthetic_cookie("desktop-session")]);
        let attempts = std::cell::Cell::new(0);
        let result: anyhow::Result<()> = with_cookies(page, None, CookieAccess::SuppliedOnly, |file| {
            attempts.set(attempts.get() + 1);
            assert!(file.is_none());
            bail!("login required")
        });
        assert!(result.is_err());
        assert_eq!(attempts.get(), 1, "an API request without cookies must not retry using the desktop session");

        let supplied = [synthetic_cookie("caller-session")];
        let result = with_cookies(page, Some(&supplied), CookieAccess::SuppliedOnly, |file| {
            let Some(file) = file else { bail!("login required") };
            let content = std::fs::read_to_string(file)?;
            assert!(content.contains("caller-session"));
            assert!(!content.contains("desktop-session"));
            Ok(())
        });
        result.unwrap();
        assert_eq!(remembered_cookies(page).unwrap()[0].value, "desktop-session");
        with_cookies(page, None, CookieAccess::Remembered, |file| {
            let Some(file) = file else { bail!("login required") };
            assert!(std::fs::read_to_string(file)?.contains("desktop-session"));
            Ok(())
        }).unwrap();
    }

    #[test]
    fn temporary_cookie_file_is_removed_on_helper_error_or_panic() {
        let supplied = [synthetic_cookie("temporary-session")];
        let path = std::cell::RefCell::new(None);
        let result: anyhow::Result<()> = with_cookies("https://cleanup.invalid", Some(&supplied), CookieAccess::SuppliedOnly, |file| {
            if let Some(file) = file {
                *path.borrow_mut() = Some(file.to_path_buf());
                bail!("helper failed");
            }
            bail!("login required")
        });
        assert!(result.is_err());
        assert!(!path.borrow().as_ref().unwrap().exists());
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: anyhow::Result<()> = with_cookies("https://cleanup.invalid", Some(&supplied), CookieAccess::SuppliedOnly, |file| {
                if let Some(file) = file {
                    *path.borrow_mut() = Some(file.to_path_buf());
                    panic!("synthetic helper panic");
                }
                bail!("login required")
            });
        }));
        assert!(panic.is_err());
        assert!(!path.borrow().as_ref().unwrap().exists());
    }

    #[test]
    fn temporary_cookie_file_is_removed_on_write_error() {
        let mut cookie = cookie_file(&[synthetic_cookie("temporary-session")]).unwrap();
        let path = cookie.path.clone();
        // A read-only handle makes write_all fail on every supported platform.
        cookie.file = Some(std::fs::File::open(&path).unwrap());
        assert!(cookie.write(b"cannot write").is_err());
        assert!(!path.exists());
    }

    #[test]
    fn cookie_file_format() {
        let list = vec![
            BrowserCookie { domain: ".instagram.com".into(), name: "sessionid".into(), value: "abc".into(), path: "/".into(), secure: true, expiration_date: Some(1900000000.5) },
            BrowserCookie { domain: "www.x.com".into(), name: "a".into(), value: "b=c".into(), path: "".into(), secure: false, expiration_date: None },
        ];
        let s = netscape_cookies(&list);
        assert!(s.starts_with("# Netscape HTTP Cookie File\n"));
        assert!(s.contains(".instagram.com\tTRUE\t/\tTRUE\t1900000000\tsessionid\tabc\n"));
        assert!(s.contains("www.x.com\tFALSE\t/\tFALSE\t2147483647\ta\tb=c\n"));
        assert!(needs_login("ERROR: [Instagram] abc: Requested content is not available, rate-limit reached or login required"));
    }
}
