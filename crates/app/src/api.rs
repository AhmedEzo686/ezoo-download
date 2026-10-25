//! Tiny local HTTP API on 127.0.0.1 used by the browser extension.
//!
//! Only the extension can talk to it: requests must carry the `X-SwiftGet`
//! header (a web page cannot add custom headers to a cross-origin request
//! without a CORS preflight, which we never approve) and any Origin header
//! must belong to a browser extension.

use serde::Deserialize;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    let mut l = FOR_BROWSER.lock().unwrap();
    l.retain(|(_, at)| at.elapsed() < Duration::from_secs(300));
    l.push((url.to_string(), std::time::Instant::now()));
}

fn take_for_browser(urls: &[String]) -> bool {
    let mut l = FOR_BROWSER.lock().unwrap();
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveRequest {
    url: String,
    /// the site's cookies, used only if it asks for a login
    #[serde(default)]
    cookie_list: Vec<swiftget_engine::ytdlp::BrowserCookie>,
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

/// Another SwiftGet is already running on this port: ask it to show itself.
pub fn notify_existing(port: u16) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(800)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let req = "POST /show HTTP/1.1\r\nHost: 127.0.0.1\r\nX-SwiftGet: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.contains("Ezoo") || buf.starts_with("HTTP/1.1 200")
}

pub fn serve(
    server: Server,
    config: impl Fn() -> String + Send + 'static,
    on_event: impl Fn(ApiEvent) + Send + 'static,
) {
    std::thread::Builder::new()
        .name("ezoo-api".into())
        .spawn(move || {
            for req in server.incoming_requests() {
                handle(req, &config, &on_event);
            }
        })
        .expect("failed to start API thread");
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

/// Video look-ups running now (each one is a yt-dlp process).
static RESOLVING: AtomicUsize = AtomicUsize::new(0);
const MAX_RESOLVING: usize = 4;

fn handle(mut req: Request, config: &impl Fn() -> String, on_event: &impl Fn(ApiEvent)) {
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
    match (req.method().clone(), path.as_str()) {
        (Method::Get, "/ping") => {
            // "ext": the add-in version that came with this program; an older
            // add-in loaded from the program's folder reloads itself
            let body = format!(
                r#"{{"app":"EzooDownload","version":"{}","ext":"{}"}}"#,
                env!("CARGO_PKG_VERSION"),
                bundled_extension_version()
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
        (Method::Post, "/show") => {
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
                let body = match swiftget_engine::ytdlp::resolve(&r.url, cookies) {
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

#[cfg(test)]
mod tests {
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
