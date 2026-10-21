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

/// Ask the server about a file: real size, final URL after redirects,
/// file name and whether it supports ranges (= resume + many connections).
pub async fn probe(
    client: &Client,
    url: &str,
    headers: &RequestHeaders,
    default_ua: &str,
) -> anyhow::Result<ProbeResult> {
    let resp = request(client, url, headers, default_ua)
        .header(RANGE, "bytes=0-0")
        .send()
        .await
        .context("could not connect to server")?;
    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED {
        bail!("the server asks for a user name and password (HTTP 401) - use \"Use authorization\" or Options > Site Logins");
    }
    if !status.is_success() {
        bail!("server answered HTTP {}", status.as_u16());
    }
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

    Ok(ProbeResult {
        final_url,
        file_name,
        total_size: if kind == DownloadKind::Hls { None } else { total_size },
        resumable: kind == DownloadKind::Http && resumable,
        content_type,
        kind,
        last_modified,
    })
}
