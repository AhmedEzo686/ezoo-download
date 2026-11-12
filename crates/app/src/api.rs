//! Tiny local HTTP API on 127.0.0.1 used by the browser extension.
//!
//! Only the extension can talk to it: requests must carry the `X-SwiftGet`
//! header (a web page cannot add custom headers to a cross-origin request
//! without a CORS preflight, which we never approve) and any Origin header
//! must belong to a browser extension.

use serde::Deserialize;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server};

/// Addresses the user chose to download with the browser itself (the server
/// refused Ezoo Download): the extension leaves them to the browser once.
static FOR_BROWSER: std::sync::Mutex<Vec<(String, std::time::Instant)>> = std::sync::Mutex::new(Vec::new());

/// Version of the browser add-in that ships with this program.
pub fn bundled_extension_version() -> String {
    serde_json::from_str::<serde_json::Value>(include_str!("../../../extension/manifest.json"))
        .ok()
        .and_then(|v| v["version"].as_str().map(str::to_string))
        .unwrap_or_default()
}

pub fn leave_to_browser(url: &str) {
    let mut l = FOR_BROWSER.lock().unwrap_or_else(|e| e.into_inner());
    l.retain(|(_, at)| at.elapsed() < Duration::from_secs(300));
    l.push((url.to_string(), std::time::Instant::now()));
}

fn take_for_browser(urls: &[String]) -> bool {
    let mut l = FOR_BROWSER.lock().unwrap_or_else(|e| e.into_inner());
    l.retain(|(_, at)| at.elapsed() < Duration::from_secs(300));
    if let Some(i) = l.iter().position(|(u, _)| urls.iter().any(|x| x == u)) {
        l.remove(i);
        return true;
    }
    false
}

#[derive(Deserialize)]
struct BrowserQuery {
    urls: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRequest {
    pub url: String,
    #[serde(default)]
    pub referrer: Option<String>,
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    /// YouTube style: separate audio stream to merge with `url`
    #[serde(default)]
    pub audio_url: Option<String>,
    /// extra headers the links need (from yt-dlp)
    #[serde(default)]
    pub headers: Option<Vec<(String, String)>>,
    /// YouTube page + quality key, so expired links can be renewed
    #[serde(default)]
    pub source_page: Option<String>,
    #[serde(default)]
    pub source_key: Option<String>,
    /// "Download a part": only from / to (seconds)
    #[serde(default)]
    pub clip: Option<ClipAsk>,
    /// a form's download: what the form sent (base64) and its kind
    #[serde(default)]
    pub post: Option<String>,
    #[serde(default)]
    pub post_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClipAsk {
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub exact: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveRequest {
    url: String,
    /// the site's cookies, used only if it asks for a login
    #[serde(default)]
    cookie_list: Vec<swiftget_engine::ytdlp::BrowserCookie>,
    /// a stream found on a page: that page
    #[serde(default)]
    referer: Option<String>,
}

/// "Download all links with Ezoo Download" from the browser's right click menu.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinksRequest {
    #[serde(default)]
    pub page: String,
    pub links: Vec<PageLink>,
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PageLink {
    pub url: String,
    #[serde(default)]
    pub text: String,
}

pub enum ApiEvent {
    Add(AddRequest),
    Links(LinksRequest),
    Show,
    /// "Download all" on a playlist page: open the playlist window with it
    Playlist(String),
    /// a .torrent file / magnet link opened from Windows (double click):
    /// the torrent window with it
    OpenTorrent(String),
    /// a browser add-in without the key asks to connect: yes / no
    Pair(std::sync::mpsc::Sender<bool>),
    /// the setup of a new version wants the program closed (it saves
    /// everything first; the downloads go on after the update)
    Quit,
    /// the Ezoo Download add-on in Firefox is older than this program
    /// (Firefox keeps its own copy): offer to put the new one in
    OldFirefoxAddon,
}

/// Ask the copy of the program that is open to close (for the setup).
/// true = it said it will.
pub fn notify_quit(port: u16) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(800)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let req = "POST /quit HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.contains("Ezoo")
}

/// What Windows may hand over to open: a magnet link or a .torrent file
/// that is there. Anything else is refused.
pub fn torrent_source_ok(s: &str) -> bool {
    let s = s.trim();
    swiftget_engine::torrent::is_magnet(s)
        || (s.to_ascii_lowercase().ends_with(".torrent") && std::path::Path::new(s).is_file())
}

/// What the extension needs to know from Options (file types to take,
/// sites to leave alone, keys, the video panel).
pub fn config_json(s: &swiftget_engine::Settings) -> String {
    let split = |t: &str| t.split_whitespace().map(|x| x.to_string()).collect::<Vec<_>>();
    serde_json::json!({
        "ok": true,
        "captureExts": split(&s.capture_exts.to_ascii_lowercase()),
        "skipSites": split(&s.skip_sites),
        "skipAddresses": s.skip_addresses,
        "skipKey": s.skip_key,
        "forceKey": s.force_key,
        "mp3Kbps": s.mp3_kbps,
        "torrents": s.torrent_main,
        "panel": {
            "enabled": s.panel_enabled,
            "skipSites": split(&s.panel_skip_sites),
            "minVideoKb": s.panel_min_video_kb,
            "minAudioKb": s.panel_min_audio_kb,
        },
    })
    .to_string()
}

pub fn bind(port: u16) -> Result<Server, String> {
    Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())
}

/// Another Ezoo Download is already running: hand it the torrent to open.
pub fn notify_open_torrent(port: u16, source: &str) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(800)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let body = serde_json::json!({ "source": source }).to_string();
    let req = format!(
        "POST /open-torrent HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        sign_headers("POST", "/open-torrent"),
        body.len()
    );
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.starts_with("HTTP/1.1 200")
}

/// Another SwiftGet is already running on this port: ask it to show itself.
/// Seconds (since 1970) when the window side of the program last moved; the
/// main timer sets it a few times a second. Far behind = the windows hang.
pub static UI_ALIVE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// a message box / file picker of ours is open (the windows wait for it)
pub static UI_WAITS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn ui_hangs() -> bool {
    use std::sync::atomic::Ordering;
    let last = UI_ALIVE.load(Ordering::Relaxed);
    last != 0 && !UI_WAITS.load(Ordering::Relaxed) && now_secs().saturating_sub(last) > 30
}

/// What the copy of the program that is open already said.
#[derive(Debug, PartialEq)]
pub enum Existing {
    /// nobody (or something else) answers on the port
    None,
    /// it brings its window to the front
    Shown,
    /// it answers, but its windows hang (this is its process id)
    Hangs(u32),
}

pub fn notify_existing(port: u16) -> Existing {
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(800)) else {
        return Existing::None;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let req = "POST /show HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    if s.write_all(req.as_bytes()).is_err() {
        return Existing::None;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    if !(buf.contains("Ezoo") || buf.starts_with("HTTP/1.1 200")) {
        return Existing::None;
    }
    let body = buf.split("\r\n\r\n").nth(1).unwrap_or_default();
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) if v["hangs"].as_bool() == Some(true) => Existing::Hangs(v["pid"].as_u64().unwrap_or(0) as u32),
        _ => Existing::Shown,
    }
}

/// The known port could not be asked: the copy that may be open is asked on
/// its own port (signed, as the add-in does) to come to the front.
pub fn show_on_own_port(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(800)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    let req = format!(
        "POST /show HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\n{}Content-Length: 0\r\nConnection: close\r\n\r\n",
        sign_headers("POST", "/show")
    );
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.starts_with("HTTP/1.1 200") && buf.contains("Ezoo")
}

/// Which of the program's two ports a request came in on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Door {
    /// the known port (Options, 9614): only "are you there", pairing, and
    /// what this program and its setup ask ("show yourself", "close")
    Front,
    /// the port chosen at random on this computer: everything, signed
    Main,
}

/// The random port the add-in talks to (0 = not open).
pub static MAIN_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
/// Changed by "Repair the browser add-in": the add-in sees it and starts again.
pub static REPAIR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn serve(
    server: Server,
    door: Door,
    config: impl Fn() -> String + Send + 'static,
    on_event: impl Fn(ApiEvent) + Send + 'static,
) {
    std::thread::Builder::new()
        .name(format!("ezoo-api-{door:?}"))
        .spawn(move || {
            for req in server.incoming_requests() {
                // one request that goes wrong must not close the door for
                // good (the program would look closed to the add-in)
                let run = std::panic::AssertUnwindSafe(|| handle(req, door, &config, &on_event));
                if std::panic::catch_unwind(run).is_err() {
                    eprintln!("a request to the add-in port failed");
                }
            }
        })
        .expect("failed to start API thread");
}

/// The random port of this computer: the one used last time when it is free,
/// else a new one. None = no port could be opened.
pub fn bind_main(last: u16) -> Option<(Server, u16)> {
    if last >= 1024 {
        if let Ok(s) = bind(last) {
            return Some((s, last));
        }
    }
    for _ in 0..40 {
        let p = 20000 + (u16::from_str_radix(&swiftget_engine::secret::random_nonce()[..4], 16).unwrap_or(1) % 40000);
        if let Ok(s) = bind(p) {
            return Some((s, p));
        }
    }
    None
}

/// The request was signed with the program's key (see secret.rs), lately,
/// and was not seen before.
fn signed(req: &Request, path: &str) -> bool {
    let Some(key) = TOKEN.get().filter(|k| !k.is_empty()) else { return false };
    let (Some(time), Some(nonce), Some(sig)) = (header(req, "X-Ezoo-Time"), header(req, "X-Ezoo-Nonce"), header(req, "X-Ezoo-Sig")) else {
        return false;
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let Ok(at) = time.parse::<i64>() else { return false };
    // made within the last minute (or a little ahead: the same clock)
    if (now - at).abs() > 60_000 || nonce.len() < 16 || nonce.len() > 64 {
        return false;
    }
    if !swiftget_engine::secret::signature_ok(key, req.method().as_str(), path, &time, &nonce, &sig) {
        return false;
    }
    // each signed request only once
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    seen.retain(|_, t| t.elapsed() < Duration::from_secs(130));
    seen.insert(nonce, std::time::Instant::now()).is_none()
}

static SEEN: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>> =
    std::sync::LazyLock::new(Default::default);

/// Headers that sign a request of this program to itself (a second copy
/// asking the open one to show a torrent).
pub fn sign_headers(method: &str, path: &str) -> String {
    let key = TOKEN.get().cloned().unwrap_or_default();
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0).to_string();
    let nonce = swiftget_engine::secret::random_nonce();
    let sig = swiftget_engine::secret::request_signature(&key, method, path, &time, &nonce);
    format!("X-Ezoo-Time: {time}\r\nX-Ezoo-Nonce: {nonce}\r\nX-Ezoo-Sig: {sig}\r\n")
}

fn is_http(u: &str) -> bool {
    u.starts_with("http://") || u.starts_with("https://")
}

fn header(req: &Request, name: &'static str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

fn reply(req: Request, code: u16, body: &str) {
    let resp = Response::from_string(body)
        .with_status_code(code)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
    let _ = req.respond(resp);
}

/// The request was made to this computer by its own name. A web page that
/// points its own domain at 127.0.0.1 (DNS rebinding) sends its domain here.
fn host_allowed(host: Option<&str>) -> bool {
    let Some(host) = host else { return true }; // HTTP/1.0: no Host at all
    let h = host.trim().to_ascii_lowercase();
    let name = if let Some(rest) = h.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        h.split(':').next().unwrap_or("")
    };
    matches!(name, "127.0.0.1" | "localhost" | "::1")
}

/// Only these may call the API (a web page has an http(s) origin).
fn origin_allowed(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some(o) => {
            o.starts_with("chrome-extension://") || o.starts_with("moz-extension://") || o.starts_with("extension://")
        }
    }
}

/// The secret key of this program (Settings::api_token), set at start.
pub static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();


/// A pairing is being asked about right now / the user said no (when).
static PAIRING: std::sync::Mutex<(bool, Option<std::time::Instant>)> = std::sync::Mutex::new((false, None));

/// After the user said no, pairing waits this long before it may ask again.
const AFTER_NO: Duration = Duration::from_secs(120);

/// An add-in without the key (Firefox, or one loaded from another folder)
/// asks for it: the user is asked in a window of the program; yes = it gets
/// the key. Asked once at a time, and not again for 10 minutes after a no.
fn pair(req: Request, on_event: &impl Fn(ApiEvent)) {
    {
        let mut p = PAIRING.lock().unwrap_or_else(|e| e.into_inner());
        let refused_lately = p.1.map(|t| t.elapsed() < AFTER_NO).unwrap_or(false);
        if p.0 || refused_lately {
            return reply(req, 429, r#"{"error":"wait"}"#);
        }
        p.0 = true;
    }
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    on_event(ApiEvent::Pair(tx));
    // the answer waits for the user (on its own thread: other requests go on)
    std::thread::spawn(move || {
        // yes / no (the window closed without yes) / no answer at all
        let answer = rx.recv_timeout(Duration::from_secs(120));
        let yes = matches!(answer, Ok(true));
        {
            let mut p = PAIRING.lock().unwrap_or_else(|e| e.into_inner());
            p.0 = false;
            // only a real "no" waits before asking again; no answer does not
            if !yes && !matches!(answer, Err(std::sync::mpsc::RecvTimeoutError::Timeout)) {
                p.1 = Some(std::time::Instant::now());
            }
        }
        if yes {
            let body = format!(
                r#"{{"ok":true,"token":"{}","port":{}}}"#,
                TOKEN.get().cloned().unwrap_or_default(),
                MAIN_PORT.load(std::sync::atomic::Ordering::Relaxed)
            );
            reply(req, 200, &body)
        } else {
            reply(req, 403, r#"{"error":"refused"}"#)
        }
    });
}

/// The Firefox add-on that asks is older than the one of this program.
/// Add-ins say their version (X-Ezoo-Ext, since 1.0.10); a Firefox add-on
/// that says nothing is from before.
fn old_firefox_addon(req: &Request) -> bool {
    is_old_firefox(
        header(req, "X-Ezoo-Browser").as_deref(),
        &header(req, "User-Agent").unwrap_or_default(),
        header(req, "X-Ezoo-Ext").as_deref(),
        &bundled_extension_version(),
    )
}

fn is_old_firefox(browser: Option<&str>, user_agent: &str, ext: Option<&str>, ours: &str) -> bool {
    let firefox = match browser {
        Some(b) => b == "firefox",
        None => user_agent.contains("Firefox/"),
    };
    if !firefox || ours.is_empty() {
        return false;
    }
    match ext {
        Some(v) => swiftget_engine::selfupdate::is_newer(ours, v.trim()),
        // no version: an add-on from before 1.0.10
        None => true,
    }
}

/// At most once every 6 hours (the user may say "later").
fn time_to_tell_old_addon() -> bool {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let last = LAST.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < 6 * 3600 {
        return false;
    }
    LAST.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok()
}

/// Video look-ups running now (each one is a yt-dlp process).
static RESOLVING: AtomicUsize = AtomicUsize::new(0);
const MAX_RESOLVING: usize = 3;

fn handle(mut req: Request, door: Door, config: &impl Fn() -> String, on_event: &impl Fn(ApiEvent)) {
    // never answer CORS preflights: web pages must not reach this API
    if *req.method() == Method::Options {
        return reply(req, 403, r#"{"error":"forbidden"}"#);
    }
    if !host_allowed(header(&req, "Host").as_deref()) {
        return reply(req, 403, r#"{"error":"forbidden"}"#);
    }
    if !origin_allowed(header(&req, "Origin").as_deref()) {
        return reply(req, 403, r#"{"error":"forbidden"}"#);
    }
    if header(&req, "X-SwiftGet").is_none() {
        return reply(req, 403, r#"{"error":"missing header"}"#);
    }

    let path = req.url().split('?').next().unwrap_or("").to_string();
    if old_firefox_addon(&req) && time_to_tell_old_addon() {
        on_event(ApiEvent::OldFirefoxAddon);
    }
    // "are you there", pairing, "show yourself" / "close" (this program and
    // its setup) are open on the known port; everything else is signed with
    // the key the add-in got from this program (another program on the
    // computer has no key), and only "where is the other port" and opening
    // a torrent may come in on the known port
    let open = matches!(path.as_str(), "/ping") || (door == Door::Front && matches!(path.as_str(), "/show" | "/quit" | "/pair"));
    if !open {
        let front_ok = matches!(path.as_str(), "/where" | "/open-torrent");
        if door == Door::Front && !front_ok {
            return reply(req, 404, r#"{"error":"not here","where":true}"#);
        }
        if !signed(&req, &path) {
            return reply(req, 401, r#"{"error":"pair","pair":true}"#);
        }
        // an add-in with the key says who it is: the browser's helper may be
        // started by it from now on (one without the key is never added)
        if let Some(id) = header(&req, "X-Ezoo-Id") {
            crate::nativehost::saw_addin(id.trim());
        }
    }
    if path == "/pair" && *req.method() == Method::Post {
        return pair(req, on_event);
    }
    if path == "/where" {
        let body = format!(r#"{{"ok":true,"port":{}}}"#, MAIN_PORT.load(std::sync::atomic::Ordering::Relaxed));
        return reply(req, 200, &body);
    }
    match (req.method().clone(), path.as_str()) {
        (Method::Get, "/ping") => {
            // "ext": the add-in version that came with this program; an older
            // add-in loaded from the program's folder reloads itself
            // "repair": "Repair the browser add-in" was used (the add-in
            // starts again on a new number)
            let body = format!(
                r#"{{"app":"EzooDownload","version":"{}","ext":"{}","repair":{}}}"#,
                env!("CARGO_PKG_VERSION"),
                bundled_extension_version(),
                REPAIR.load(std::sync::atomic::Ordering::Relaxed)
            );
            reply(req, 200, &body)
        }
        (Method::Get, "/config") => reply(req, 200, &config()),
        (Method::Post, "/links") => {
            let mut body = String::new();
            if req.as_reader().take(16 << 20).read_to_string(&mut body).is_err() {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            }
            match serde_json::from_str::<LinksRequest>(&body) {
                Ok(mut l) => {
                    l.links.retain(|x| is_http(&x.url));
                    on_event(ApiEvent::Links(l));
                    reply(req, 200, r#"{"ok":true}"#)
                }
                Err(_) => reply(req, 400, r#"{"error":"invalid request"}"#),
            }
        }
        (Method::Post, "/browser") => {
            // should the browser keep this download? (see leave_to_browser)
            let mut body = String::new();
            if req.as_reader().take(1 << 16).read_to_string(&mut body).is_err() {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            }
            let keep = serde_json::from_str::<BrowserQuery>(&body).map(|q| take_for_browser(&q.urls)).unwrap_or(false);
            reply(req, 200, if keep { r#"{"browser":true}"# } else { r#"{"browser":false}"# })
        }
        (Method::Post, "/open-torrent") => {
            let mut body = String::new();
            if req.as_reader().take(1 << 16).read_to_string(&mut body).is_err() {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            }
            match serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| v["source"].as_str().map(String::from)) {
                Some(s) if torrent_source_ok(&s) => {
                    on_event(ApiEvent::OpenTorrent(s.trim().to_string()));
                    reply(req, 200, r#"{"ok":true}"#)
                }
                _ => reply(req, 400, r#"{"error":"invalid request"}"#),
            }
        }
        (Method::Post, "/quit") => {
            on_event(ApiEvent::Quit);
            reply(req, 200, r#"{"ok":true,"app":"Ezoo"}"#)
        }
        (Method::Post, "/show") => {
            if ui_hangs() {
                let body = format!(r#"{{"ok":true,"app":"Ezoo","hangs":true,"pid":{}}}"#, std::process::id());
                return reply(req, 200, &body);
            }
            on_event(ApiEvent::Show);
            reply(req, 200, r#"{"ok":true,"app":"Ezoo"}"#)
        }
        (Method::Post, "/playlist") => {
            let mut body = String::new();
            if req.as_reader().take(1 << 16).read_to_string(&mut body).is_err() {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            }
            match serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| v["url"].as_str().map(String::from)) {
                Some(url) if is_http(&url) => {
                    on_event(ApiEvent::Playlist(url));
                    reply(req, 200, r#"{"ok":true}"#)
                }
                _ => reply(req, 400, r#"{"error":"invalid request"}"#),
            }
        }
        (Method::Post, "/add") => {
            let mut body = String::new();
            if req.as_reader().take(1 << 20).read_to_string(&mut body).is_err() {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            }
            match serde_json::from_str::<AddRequest>(&body) {
                Ok(add)
                    if (is_http(&add.url) || swiftget_engine::torrent::is_magnet(&add.url))
                        && add.audio_url.as_deref().map(is_http).unwrap_or(true) =>
                {
                    on_event(ApiEvent::Add(add));
                    reply(req, 200, r#"{"ok":true}"#)
                }
                _ => reply(req, 400, r#"{"error":"invalid request"}"#),
            }
        }
        (Method::Post, "/check") => {
            // a download the browser started: can the program get it too?
            // Links that work once only (the browser just used them) or
            // only with the page's own sign-in (ChatGPT, iLovePDF...) stay
            // in the browser instead of failing here
            let mut body = String::new();
            if req.as_reader().take(1 << 20).read_to_string(&mut body).is_err() {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            }
            let Ok(add) = serde_json::from_str::<AddRequest>(&body) else {
                return reply(req, 400, r#"{"error":"invalid request"}"#);
            };
            if !is_http(&add.url) {
                return reply(req, 400, r#"{"error":"not a web address"}"#);
            }
            std::thread::spawn(move || {
                let ok = can_fetch(&add);
                reply(req, 200, if ok { r#"{"ok":true}"# } else { r#"{"ok":false}"# });
            });
        }
        (Method::Post, "/resolve") | (Method::Post, "/youtube") => {
            // download choices for any page with a video (yt-dlp, a few
            // seconds), answered from its own thread so nothing else waits
            let mut body = String::new();
            if req.as_reader().take(4 << 20).read_to_string(&mut body).is_err() {
                return reply(req, 400, r#"{"error":"bad body"}"#);
            }
            let Ok(r) = serde_json::from_str::<ResolveRequest>(&body) else {
                return reply(req, 400, r#"{"error":"invalid request"}"#);
            };
            if !is_http(&r.url) {
                return reply(req, 400, r#"{"error":"not a web address"}"#);
            }
            if RESOLVING.fetch_add(1, Ordering::SeqCst) >= MAX_RESOLVING {
                RESOLVING.fetch_sub(1, Ordering::SeqCst);
                return reply(req, 429, r#"{"ok":false,"error":"busy, try again in a moment"}"#);
            }
            std::thread::spawn(move || {
                struct Done;
                impl Drop for Done {
                    fn drop(&mut self) {
                        RESOLVING.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                let _done = Done;
                let cookies = (!r.cookie_list.is_empty()).then_some(r.cookie_list.as_slice());
                let referer = r.referer.as_deref().filter(|u| is_http(u));
                let body = match swiftget_engine::ytdlp::resolve_for(&r.url, cookies, referer) {
                    Ok(info) => serde_json::json!({
                        "ok": true, "title": info.title, "site": info.site, "options": info.options
                    }),
                    Err(e) => serde_json::json!({ "ok": false, "error": crate::err_text(&format!("{e:#}")) }),
                };
                reply(req, 200, &body.to_string());
            });
        }
        _ => reply(req, 404, r#"{"error":"not found"}"#),
    }
}

/// Whether the program gets a file at `add.url` with the browser's sign-in
/// (a short look, a few seconds at most; when it can't tell, yes).
fn can_fetch(add: &AddRequest) -> bool {
    let mut headers = swiftget_engine::RequestHeaders {
        referer: add.referrer.clone(),
        cookies: add.cookies.clone(),
        user_agent: add.user_agent.clone(),
        extra: Vec::new(),
        auth: None,
        post: add.post.clone(),
        post_type: add.post_type.clone(),
    };
    if let Some(list) = &add.headers {
        swiftget_engine::ytdlp::apply_headers(&mut headers, list);
    }
    // one lasting runtime for these looks: the answer a look keeps for the
    // download stays open only while the runtime it came on runs
    static RT: std::sync::LazyLock<Option<tokio::runtime::Runtime>> =
        std::sync::LazyLock::new(|| tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().ok());
    let Some(rt) = RT.as_ref() else { return true };
    let Ok(client) = swiftget_engine::http::build_client_with(&swiftget_engine::http::current_proxy()) else { return true };
    let url = add.url.clone();
    rt.block_on(async move {
        let look = swiftget_engine::http::probe_quick(&client, &url, &headers, swiftget_engine::config::DEFAULT_USER_AGENT);
        match tokio::time::timeout(std::time::Duration::from_millis(2500), look).await {
            // a sign-in page instead of the file
            Ok(Ok(p)) => !p.is_web_page(),
            Ok(Err(e)) => match swiftget_engine::http::http_status_of(&e) {
                Some(code) => !matches!(code, 400 | 401 | 403 | 404 | 405 | 410),
                None => {
                    let text = format!("{e:#}");
                    !(text.contains("HTTP 40") || text.contains("HTTP 410") || text.contains("user name and password"))
                }
            },
            // slow to answer: the program takes it (it waits longer)
            Err(_) => true,
        }
    })
}

#[cfg(test)]
mod tests {

    #[test]
    fn an_old_firefox_addon_is_told() {
        use super::is_old_firefox;
        let ff = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:145.0) Gecko/20100101 Firefox/145.0";
        let chrome = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
        // an add-on from before (no version): Firefox only
        assert!(is_old_firefox(None, ff, None, "1.0.10"));
        assert!(!is_old_firefox(None, chrome, None, "1.0.10"));
        // the add-on says its version
        assert!(is_old_firefox(Some("firefox"), ff, Some("1.0.9"), "1.0.10"));
        assert!(!is_old_firefox(Some("firefox"), ff, Some("1.0.10"), "1.0.10"));
        assert!(!is_old_firefox(Some("firefox"), ff, Some("1.0.11"), "1.0.10"));
        assert!(!is_old_firefox(Some("chromium"), chrome, Some("1.0.8"), "1.0.10"));
        assert!(!is_old_firefox(None, ff, None, ""));
    }

    #[test]
    fn the_two_doors() {
        use std::io::{Read, Write};
        let _ = TOKEN.set("a".repeat(64));
        let front = bind(0).unwrap();
        let fport = front.server_addr().to_ip().unwrap().port();
        let main = bind(0).unwrap();
        let mport = main.server_addr().to_ip().unwrap().port();
        MAIN_PORT.store(mport, std::sync::atomic::Ordering::Relaxed);
        serve(front, Door::Front, || "{}".to_string(), |_| {});
        serve(main, Door::Main, || r#"{"ok":true}"#.to_string(), |_| {});
        let ask = |port: u16, path: &str, signed: Option<String>| -> String {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let sig = signed.unwrap_or_default();
            let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\n{sig}Connection: close\r\n\r\n");
            s.write_all(req.as_bytes()).unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            out
        };
        // unsigned: refused on the real port; the known port has no /config
        assert!(ask(mport, "/config", None).starts_with("HTTP/1.1 401"));
        assert!(ask(fport, "/config", Some(sign_headers("GET", "/config"))).starts_with("HTTP/1.1 404"));
        // signed: taken, once
        let h = sign_headers("GET", "/config");
        assert!(ask(mport, "/config", Some(h.clone())).starts_with("HTTP/1.1 200"));
        assert!(ask(mport, "/config", Some(h)).starts_with("HTTP/1.1 401"), "a signed request is taken only once");
        // signed for another path: refused
        assert!(ask(mport, "/config", Some(sign_headers("GET", "/links"))).starts_with("HTTP/1.1 401"));
        // where the real port is: only with a signature
        assert!(ask(fport, "/where", None).starts_with("HTTP/1.1 401"));
        assert!(ask(fport, "/where", Some(sign_headers("GET", "/where"))).contains(&format!("\"port\":{mport}")));
        // "are you there" stays open on both
        assert!(ask(fport, "/ping", None).starts_with("HTTP/1.1 200"));
    }

    /// The browser's helper (crates/host) talks to the program as the add-in
    /// does: run with EZOO_HOST_BIN = the built helper.
    #[test]
    fn the_helper_gets_through() {
        use std::io::{Read, Write};
        let Some(bin) = std::env::var_os("EZOO_HOST_BIN") else { return };
        let _ = TOKEN.set("a".repeat(64));
        let token = TOKEN.get().unwrap().clone();
        let main = bind(0).unwrap();
        let mport = main.server_addr().to_ip().unwrap().port();
        serve(main, Door::Main, || r#"{"ok":true,"from":"config"}"#.to_string(), |_| {});
        let dir = std::env::temp_dir().join(format!("ezoo-helper-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let host = dir.join("ezoo-host");
        std::fs::copy(&bin, &host).unwrap();
        std::fs::write(dir.join("settings.json"), format!(r#"{{"api_port":{mport},"api_token":"{token}"}}"#)).unwrap();
        std::fs::write(dir.join("app.json"), serde_json::json!({ "settings": dir.join("settings.json") }).to_string()).unwrap();
        let mut child = std::process::Command::new(&host)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let mut ask = |v: serde_json::Value| -> serde_json::Value {
            let d = v.to_string();
            input.write_all(&(d.len() as u32).to_ne_bytes()).unwrap();
            input.write_all(d.as_bytes()).unwrap();
            input.flush().unwrap();
            let mut len = [0u8; 4];
            output.read_exact(&mut len).unwrap();
            let mut buf = vec![0u8; u32::from_ne_bytes(len) as usize];
            output.read_exact(&mut buf).unwrap();
            serde_json::from_slice(&buf).unwrap()
        };
        let a = ask(serde_json::json!({ "id": 1, "method": "GET", "path": "/config", "ms": 3000 }));
        assert_eq!(a["id"], 1);
        assert_eq!(a["status"], 200, "{a}");
        assert!(a["body"].as_str().unwrap().contains("config"));
        let a = ask(serde_json::json!({ "id": 2, "method": "GET", "path": "/ping", "ms": 3000 }));
        assert_eq!(a["status"], 200);
        assert!(a["body"].as_str().unwrap().contains("EzooDownload"));
        let a = ask(serde_json::json!({ "id": 3, "method": "POST", "path": "/nothing", "body": "{}", "ms": 3000 }));
        assert_eq!(a["status"], 404);
        // a wrong key: the program says no
        std::fs::write(dir.join("settings.json"), format!(r#"{{"api_port":{mport},"api_token":"{}"}}"#, "b".repeat(64))).unwrap();
        let a = ask(serde_json::json!({ "id": 4, "method": "GET", "path": "/config", "ms": 3000 }));
        assert_eq!(a["status"], 401);
        drop(input);
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn links_the_program_can_not_get_stay_in_the_browser() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in l.incoming().flatten().take(20) {
                let mut s = s;
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let answer = if head.starts_with("GET /gone") {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else if head.starts_with("GET /signin") {
                    "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-9/10\r\nContent-Length: 10\r\nContent-Type: application/zip\r\nConnection: close\r\n\r\n0123456789".to_string()
                };
                let _ = s.write_all(answer.as_bytes());
            }
        });
        let ask = |path: &str| {
            let a: AddRequest = serde_json::from_str(&format!(r#"{{"url":"http://127.0.0.1:{port}{path}"}}"#)).unwrap();
            can_fetch(&a)
        };
        assert!(ask("/file.zip"));
        assert!(!ask("/gone/file.zip"), "a link that works once only");
        assert!(!ask("/signin/file.zip"), "a link that needs the page's sign-in");
    }

    #[test]
    fn a_part_of_a_video_from_the_add_in() {
        let a: AddRequest = serde_json::from_str(r#"{"url":"https://x.test/v.mp4","clip":{"start":5,"end":9.5,"exact":true}}"#).unwrap();
        let c = a.clip.unwrap();
        assert_eq!((c.start, c.end, c.exact), (5.0, 9.5, true));
        let b: AddRequest = serde_json::from_str(r#"{"url":"https://x.test/v.mp4","clip":null}"#).unwrap();
        assert!(b.clip.is_none());
    }

    #[test]
    fn signed_requests_only_once() {
        let _ = TOKEN.set("a".repeat(64));
        let h = sign_headers("POST", "/add");
        assert!(h.contains("X-Ezoo-Sig: ") && !h.contains(&"a".repeat(64)), "the key itself is never sent");
    }

    use super::*;

    #[test]
    fn only_this_computer_by_its_own_name() {
        for ok in [None, Some("127.0.0.1:9614"), Some("localhost:9614"), Some("127.0.0.1"), Some("[::1]:9614")] {
            assert!(host_allowed(ok), "{ok:?}");
        }
        // DNS rebinding: the attacker's own name pointed at 127.0.0.1
        for bad in [Some("evil.example:9614"), Some("127.0.0.1.evil.example"), Some("localhost.evil:9614"), Some("")] {
            assert!(!host_allowed(bad), "{bad:?}");
        }
    }

    #[test]
    fn only_browser_add_ins() {
        assert!(origin_allowed(None));
        assert!(origin_allowed(Some("chrome-extension://abcdef")));
        assert!(origin_allowed(Some("moz-extension://1234-5678")));
        assert!(!origin_allowed(Some("https://evil.example")));
        assert!(!origin_allowed(Some("null")));
    }
}
