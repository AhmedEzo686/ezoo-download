//! HTTP client, request building and probing a URL.

use crate::filename;
use crate::limiter::SpeedLimiter;
use crate::model::{DownloadKind, ProbeResult, RequestHeaders};
use anyhow::{bail, Context};
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_RANGE, CONTENT_TYPE,
    COOKIE, RANGE, REFERER, USER_AGENT,
};
use reqwest::{Client, RequestBuilder, StatusCode};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Live counters of a running download, read by the UI.
#[derive(Default)]
pub struct Progress {
    pub bytes: AtomicU64,
    pub connections: AtomicU32,
    pub parts_total: AtomicU32,
    pub parts_done: AtomicU32,
    pub assembling: AtomicBool,
    /// the connection dropped: waiting for the internet to come back
    pub waiting: AtomicBool,
    /// woken when data flows again, so every waiting part retries at once
    pub back_online: tokio::sync::Notify,
}

impl Progress {
    /// Data arrived: the connection works (again).
    pub fn online(&self) {
        if self.waiting.swap(false, Ordering::Relaxed) {
            self.back_online.notify_waiters();
        }
    }
}

/// The connection broke while receiving (stall, early close).
#[derive(Debug)]
pub struct NetDrop(pub &'static str);
impl std::fmt::Display for NetDrop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for NetDrop {}

/// The server answered with an error status.
#[derive(Debug)]
pub struct HttpStatus(pub u16);
impl std::fmt::Display for HttpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "server answered HTTP {}", self.0)
    }
}
impl std::error::Error for HttpStatus {}

/// A problem of the connection (no internet, DNS, reset, timeout), not of
/// the file or the link: worth waiting for and trying again without limit.
pub fn is_network_error(e: &anyhow::Error) -> bool {
    use std::io::ErrorKind as K;
    e.chain().any(|c| {
        if c.is::<NetDrop>() {
            return true;
        }
        if let Some(r) = c.downcast_ref::<reqwest::Error>() {
            return r.status().is_none() && !r.is_builder() && !r.is_redirect()
                && (r.is_connect() || r.is_timeout() || r.is_request() || r.is_body() || r.is_decode());
        }
        if let Some(io) = c.downcast_ref::<std::io::Error>() {
            return matches!(
                io.kind(),
                K::ConnectionReset | K::ConnectionAborted | K::ConnectionRefused | K::NotConnected
                    | K::TimedOut | K::BrokenPipe | K::UnexpectedEof | K::HostUnreachable
                    | K::NetworkUnreachable | K::NetworkDown
            );
        }
        false
    })
}

/// The server's status code, if the error is one.
pub fn http_status_of(e: &anyhow::Error) -> Option<u16> {
    e.chain().find_map(|c| c.downcast_ref::<HttpStatus>().map(|s| s.0))
}

/// How long to wait before trying again after `n` failures in a row while
/// the connection is down: quick at first, then every 10 seconds.
pub fn reconnect_delay(n: u32) -> Duration {
    Duration::from_secs(match n {
        0 | 1 => 2,
        2 => 3,
        3 => 5,
        4 => 8,
        _ => 10,
    })
}

/// Counts an open connection for as long as it lives.
pub struct ConnGuard(Arc<Progress>);
impl ConnGuard {
    pub fn new(p: &Arc<Progress>) -> Self {
        p.connections.fetch_add(1, Ordering::Relaxed);
        ConnGuard(p.clone())
    }
}
impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Things every transfer needs.
pub struct Shared {
    /// replaced when the proxy settings change
    client: RwLock<Client>,
    pub limiter: SpeedLimiter,
}

impl Shared {
    pub fn new(client: Client, limiter: SpeedLimiter) -> Shared {
        Shared { client: RwLock::new(client), limiter }
    }
    /// The HTTP client (cheap to clone: it is shared inside).
    pub fn client(&self) -> Client {
        self.client.read().unwrap().clone()
    }
    pub fn set_client(&self, c: Client) {
        *self.client.write().unwrap() = c;
    }
}

/// Proxy part of the settings (Options > Proxy / Socks).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProxyConfig {
    /// 0 none, 1 system, 2 manual
    pub mode: u32,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub bypass: String,
}

impl ProxyConfig {
    pub fn from_settings(s: &crate::config::Settings) -> ProxyConfig {
        ProxyConfig {
            mode: s.proxy_mode,
            host: s.proxy_host.trim().to_string(),
            port: s.proxy_port,
            user: s.proxy_user.clone(),
            password: s.proxy_password.clone(),
            bypass: s.proxy_bypass.clone(),
        }
    }
}

pub fn build_client() -> anyhow::Result<Client> {
    build_client_with(&ProxyConfig { mode: 1, ..Default::default() })
}

pub fn build_client_with(proxy: &ProxyConfig) -> anyhow::Result<Client> {
    let mut b = Client::builder()
        // Real separate TCP connections. With HTTP/2 all our
        // "connections" would share one TCP connection and be as slow as one.
        .http1_only()
        .tcp_nodelay(true)
        .connect_timeout(Duration::from_secs(20))
        .pool_max_idle_per_host(64)
        .redirect(reqwest::redirect::Policy::limited(15));
    match proxy.mode {
        // 1 = system settings: reqwest reads them by itself
        0 => b = b.no_proxy(),
        2 if !proxy.host.is_empty() => {
            let host = proxy.host.trim_start_matches("http://").trim_start_matches("https://");
            let url = format!("http://{host}:{}", proxy.port);
            let mut p = reqwest::Proxy::all(&url).context("the proxy address is not valid")?;
            if !proxy.user.is_empty() {
                p = p.basic_auth(&proxy.user, &proxy.password);
            }
            let bypass = proxy.bypass.split_whitespace().collect::<Vec<_>>().join(",");
            p = p.no_proxy(reqwest::NoProxy::from_string(&bypass));
            b = b.proxy(p);
        }
        _ => {}
    }
    Ok(b.build()?)
}

/// GET request carrying the browser's headers.
pub fn request(client: &Client, url: &str, h: &RequestHeaders, default_ua: &str) -> RequestBuilder {
    let ua = h
        .user_agent
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(default_ua);
    // identity: ranges must refer to the raw bytes, never a compressed body
    let mut rb = client
        .get(url)
        .header(USER_AGENT, ua)
        .header(ACCEPT, "*/*")
        .header(ACCEPT_ENCODING, "identity");
    if let Some(r) = h.referer.as_deref().filter(|s| !s.is_empty()) {
        rb = rb.header(REFERER, r);
    }
    if let Some(c) = h.cookies.as_deref().filter(|s| !s.is_empty()) {
        rb = rb.header(COOKIE, c);
    }
    for (k, v) in &h.extra {
        rb = rb.header(k.as_str(), v.as_str());
    }
    if let Some((user, pass)) = h.auth.as_ref().filter(|(u, _)| !u.is_empty()) {
        rb = rb.basic_auth(user, Some(pass));
    }
    rb
}

fn looks_like_hls(url: &str, content_type: Option<&str>) -> bool {
    let ct = content_type.unwrap_or("").to_ascii_lowercase();
    if ct.contains("mpegurl") {
        return true;
    }
    url::Url::parse(url)
        .map(|u| u.path().to_ascii_lowercase().ends_with(".m3u8"))
        .unwrap_or(false)
}

/// Ways to ask again when a server refuses the request as the browser sent
/// it: some sites refuse a byte range, or cookies / a referrer that belong to
/// the browser's own session. (with range?, headers)
fn fallbacks(h: &RequestHeaders) -> Vec<(bool, RequestHeaders)> {
    let mut out = vec![(true, h.clone()), (false, h.clone())];
    let has = |s: &Option<String>| s.as_deref().map(|v| !v.is_empty()).unwrap_or(false);
    if has(&h.cookies) {
        let mut n = h.clone();
        n.cookies = None;
        out.push((true, n.clone()));
        out.push((false, n));
    }
    if has(&h.cookies) || has(&h.referer) || has(&h.user_agent) {
        let mut n = h.clone();
        n.cookies = None;
        n.referer = None;
        n.user_agent = None;
        out.push((true, n.clone()));
        out.push((false, n));
    }
    out
}

/// Ask the server about a file: real size, final URL after redirects,
/// file name and whether it supports ranges (= resume + many connections).
pub async fn probe(
    client: &Client,
    url: &str,
    headers: &RequestHeaders,
    default_ua: &str,
) -> anyhow::Result<ProbeResult> {
    let mut first_refusal: Option<u16> = None;
    let mut found = None;
    for (with_range, h) in fallbacks(headers) {
        let mut rb = request(client, url, &h, default_ua);
        if with_range {
            rb = rb.header(RANGE, "bytes=0-0");
        }
        let resp = rb.send().await.context("could not connect to server")?;
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED {
            bail!("the server asks for a user name and password (HTTP 401) - use \"Use authorization\" or Options > Site Logins");
        }
        // refused (forbidden, range not allowed, too many requests...): try
        // the next way; anything else is the server's real answer
        if matches!(status.as_u16(), 403 | 406 | 416 | 429) {
            first_refusal.get_or_insert(status.as_u16());
            continue;
        }
        if !status.is_success() {
            bail!("server answered HTTP {}", status.as_u16());
        }
        let changed = h != *headers;
        found = Some((resp, with_range, changed.then_some(h)));
        break;
    }
    let Some((resp, with_range, headers_used)) = found else {
        bail!("server answered HTTP {}", first_refusal.unwrap_or(403));
    };
    let status = resp.status();
    let final_url = resp.url().to_string();
    let h = resp.headers();
    let content_type = h
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let last_modified = h
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| chrono::DateTime::parse_from_rfc2822(s.trim()).ok())
        .map(|d| d.with_timezone(&chrono::Utc));

    let (total_size, resumable) = if status == StatusCode::PARTIAL_CONTENT {
        // "bytes 0-0/123456"
        let total = h
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.rsplit('/').next())
            .and_then(|s| s.trim().parse::<u64>().ok());
        (total, total.is_some())
    } else {
        let accepts = h
            .get(ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false);
        let len = resp.content_length();
        (len, accepts && len.is_some())
    };

    let name = h
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(filename::from_content_disposition)
        .or_else(|| filename::from_url(&final_url))
        .or_else(|| filename::from_url(url))
        .unwrap_or_else(|| "download".into());
    let kind = if looks_like_hls(&final_url, content_type.as_deref()) {
        DownloadKind::Hls
    } else {
        DownloadKind::Http
    };
    let mut file_name = filename::sanitize(&name);
    if kind == DownloadKind::Hls {
        // the stream gets joined into one .ts video file
        let (stem, ext) = filename::split_ext(&file_name);
        if ext.eq_ignore_ascii_case("m3u8") || ext.is_empty() {
            file_name = format!("{stem}.ts");
        }
    } else {
        file_name = filename::ensure_extension(file_name, content_type.as_deref());
    }
    drop(resp);

    // a server that refused a byte range gets one plain connection
    let resumable = resumable && with_range;
    Ok(ProbeResult {
        headers_used,
        final_url,
        file_name,
        total_size: if kind == DownloadKind::Hls { None } else { total_size },
        resumable: kind == DownloadKind::Http && resumable,
        content_type,
        kind,
        last_modified,
    })
}

#[cfg(test)]
mod refused_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A server that answers 403 to the browser's cookie and to byte ranges.
    async fn picky_server() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                    let refused = req.contains("\r\ncookie:") || req.contains("\r\nrange:");
                    let resp = if refused {
                        "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                    } else {
                        "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\nhello".to_string()
                    };
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}/tool.bin")
    }

    #[tokio::test]
    async fn probe_falls_back_when_refused() {
        let url = picky_server().await;
        let client = Client::builder().no_proxy().build().unwrap();
        let headers = RequestHeaders { cookies: Some("cf=1".into()), ..Default::default() };
        let p = probe(&client, &url, &headers, "UA").await.unwrap();
        assert_eq!(p.total_size, Some(5));
        assert!(!p.resumable, "the server refuses ranges: one plain connection");
        let used = p.headers_used.expect("it worked only without the cookie");
        assert!(used.cookies.is_none());
    }
}
