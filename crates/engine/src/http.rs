//! HTTP client, request building and probing a URL.

use crate::sync::{LockOk, RwOk};
use crate::filename;
use crate::limiter::SpeedLimiter;
use crate::model::{DownloadKind, ProbeResult, RequestHeaders};
use anyhow::{bail, Context};
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, CONTENT_DISPOSITION, CONTENT_RANGE, CONTENT_TYPE,
    COOKIE, RANGE, REFERER, USER_AGENT,
};
use reqwest::{Client, RequestBuilder, StatusCode};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Live counters of a running download, read by the UI.
#[derive(Default)]
pub struct Progress {
    pub bytes: AtomicU64,
    pub connections: AtomicU32,
    /// torrents: how many of the connected people have the whole file
    pub seeders: AtomicU32,
    pub parts_total: AtomicU32,
    pub parts_done: AtomicU32,
    pub assembling: AtomicBool,
    /// the connection dropped: waiting for the internet to come back
    pub waiting: AtomicBool,
    /// woken when data flows again, so every waiting part retries at once
    pub back_online: tokio::sync::Notify,
    /// connections the user wants now (0 = as many as the download began
    /// with): changed while it runs, it gets more / fewer at once
    pub want_connections: AtomicU32,
    /// when the server last made it take fewer connections (seconds since
    /// 1970): a while without that, one more is tried again
    pub lowered_at: std::sync::atomic::AtomicU64,
    /// after the data: what it does (Step code, 0 = nothing) and how far (per mille)
    pub step: std::sync::atomic::AtomicU8,
    pub step_done: AtomicU32,
}

impl Progress {
    /// Now doing `step` (None: nothing more).
    pub fn set_step(&self, step: Option<crate::model::Step>) {
        self.step.store(step.map(|s| s.code()).unwrap_or(0), Ordering::Relaxed);
        self.step_done.store(0, Ordering::Relaxed);
    }
    /// How far the step is (0..1).
    pub fn step_at(&self, f: f64) {
        self.step_done.store((f.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
    }

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

/// A failure of the server or the transfer (busy server, "HTTP 503", a
/// connection that broke off early) that may well go away by itself: the
/// download tries again later on its own. Not: a link that expired or a
/// file that was removed (401/403/404/410), a login, a full disk.
pub fn is_passing_error(e: &anyhow::Error) -> bool {
    if is_network_error(e) {
        return true;
    }
    if let Some(s) = http_status_of(e) {
        return s == 408 || s == 425 || s == 429 || (500..600).contains(&s);
    }
    let text = format!("{e:#}").to_ascii_lowercase();
    let lasting = [
        "expired", "removed", "login", "age check", "not enough", "no room", "disk", "space",
        "access is denied", "permission", "bitlocker", "not found", "did not accept the byte range",
        "web page", "missing", "cannot", "can't", "invalid", "changed since",
    ];
    if lasting.iter().any(|w| text.contains(w)) {
        return false;
    }
    ["closed early", "ended before", "timed out", "reset", "broken pipe", "unexpected eof", "connection"]
        .iter()
        .any(|w| text.contains(w))
}

/// The server stopped taking the link (401 / 403 / 410, an expired link):
/// a new link is fetched the way the first one was (the address the
/// download came from, or the video's page), checked, and the download goes
/// on with it from where it was.
/// The site wants a "not a robot" check in the browser first.
pub const CAPTCHA: &str = "the site asks to confirm you are not a robot: open the link in the browser, confirm, then resume the download";

pub fn wants_captcha(e: &anyhow::Error) -> bool {
    format!("{e:#}").contains(CAPTCHA)
}

pub fn is_link_gone(e: &anyhow::Error) -> bool {
    if wants_captcha(e) {
        return false;
    }
    if matches!(http_status_of(e), Some(401 | 403 | 410)) {
        return true;
    }
    let text = format!("{e:#}").to_ascii_lowercase();
    text.contains("link has expired") || text.contains("not the same file")
}

/// How long to wait before the next try to get a working link: 2 s, 5 s,
/// 15 s, 30 s, 1 min, 2 min.
pub fn relink_delay(n: u32) -> Duration {
    Duration::from_secs(match n {
        0 | 1 => 2,
        2 => 5,
        3 => 15,
        4 => 30,
        5 => 60,
        _ => 120,
    })
}

/// How long to wait before the next try after a passing failure: soon at
/// first (a download that stood still for minutes with "trying again in
/// 10 min" was started by hand with Pause / Resume): 3 s, 5 s, 10 s, 20 s,
/// 30 s, then every minute.
pub fn retry_delay(n: u32) -> Duration {
    Duration::from_secs(match n {
        0 | 1 => 3,
        2 => 5,
        3 => 10,
        4 => 20,
        5 => 30,
        _ => 60,
    })
}

/// One "Content-Range" header of a 206 answer, e.g. "bytes 100-199/1000".
/// `total` is `None` for the open-ended "bytes 100-199/*" form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentRange {
    pub first: u64,
    pub last: u64,
    pub total: Option<u64>,
}

/// Parse a "Content-Range" value like "bytes 0-0/123456". `None` when the
/// unit, the range or the total is missing: the "bytes */1000" shape of a
/// 416 answer and free-form junk do not describe a range of bytes, and a
/// response that carries one must never be treated as a range answer.
pub fn parse_content_range(value: &str) -> Option<ContentRange> {
    let (unit, rest) = value.trim().split_once(' ')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (range, total) = rest.trim().split_once('/')?;
    let (first, last) = range.trim().split_once('-')?;
    let first = first.trim().parse::<u64>().ok()?;
    let last = last.trim().parse::<u64>().ok()?;
    if last < first {
        return None;
    }
    let total = match total.trim() {
        "*" => None,
        n => Some(n.parse::<u64>().ok()?),
    };
    Some(ContentRange { first, last, total })
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
        self.client.read_ok().clone()
    }
    pub fn set_client(&self, c: Client) {
        *self.client.write_ok() = c;
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

/// How long a server may take to answer a request (or to send the next
/// piece of data) before the request is given up and made again.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

pub fn build_client() -> anyhow::Result<Client> {
    build_client_with(&ProxyConfig { mode: 1, ..Default::default() })
}

pub fn build_client_with(proxy: &ProxyConfig) -> anyhow::Result<Client> {
    // the IPv4 copy (see `client_for`) follows the same proxy
    {
        let mut last = LAST_PROXY.lock_ok();
        if *last != *proxy {
            *last = proxy.clone();
            *V4_CLIENT.lock_ok() = None;
        }
    }
    build_client_family(proxy, false)
}

/// Many file sites give a link for "your IP" (the one the browser had on
/// their page). A computer with IPv6 may reach the file's server on its IPv6
/// address while the page saw its IPv4 one: the server answers 403 to the
/// program, never to the browser or to programs that use IPv4 only. Such a
/// server (refusing, then giving the file over IPv4) is asked over IPv4 from
/// then on.
static V4_HOSTS: std::sync::LazyLock<Mutex<std::collections::HashSet<String>>> = std::sync::LazyLock::new(Default::default);
static LAST_PROXY: std::sync::LazyLock<Mutex<ProxyConfig>> =
    std::sync::LazyLock::new(|| Mutex::new(ProxyConfig { mode: 1, ..Default::default() }));
static V4_CLIENT: Mutex<Option<Client>> = Mutex::new(None);

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(|h| h.to_ascii_lowercase())
}

fn v4_client() -> Option<Client> {
    let mut c = V4_CLIENT.lock_ok();
    if c.is_none() {
        let proxy = LAST_PROXY.lock_ok().clone();
        // through a proxy of one's own: the proxy decides
        if proxy.mode == 2 && !proxy.host.is_empty() {
            return None;
        }
        *c = build_client_family(&proxy, true).ok();
    }
    c.clone()
}

/// The client to ask `url` with: IPv4 only for servers that need it.
pub fn client_for(client: &Client, url: &str) -> Client {
    let v4 = host_of(url).is_some_and(|h| V4_HOSTS.lock_ok().contains(&h));
    if v4 {
        if let Some(c) = v4_client() {
            return c;
        }
    }
    client.clone()
}

/// The proxy the helper programs (video pages, cuts, torrents) go through,
/// as Options set it: they read it from their surroundings.
static TOOL_PROXY: std::sync::LazyLock<Mutex<ProxyConfig>> =
    std::sync::LazyLock::new(|| Mutex::new(ProxyConfig { mode: 1, ..Default::default() }));

const PROXY_VARS: [&str; 6] = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"];

/// The surroundings a helper program is started with so that it uses the
/// proxy of Options: (name, value), None = taken away.
pub fn proxy_env() -> Vec<(&'static str, Option<String>)> {
    let p = TOOL_PROXY.lock_ok().clone();
    env_for(&p)
}

fn env_for(p: &ProxyConfig) -> Vec<(&'static str, Option<String>)> {
    match p.mode {
        // no proxy: none from the surroundings either
        0 => {
            let mut v: Vec<_> = PROXY_VARS.iter().map(|k| (*k, None)).collect();
            v.push(("NO_PROXY", Some("*".into())));
            v.push(("no_proxy", Some("*".into())));
            v
        }
        2 if !p.host.is_empty() => {
            let host = p.host.trim_start_matches("http://").trim_start_matches("https://").trim_end_matches('/');
            let enc = |s: &str| percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string();
            let login = if p.user.is_empty() { String::new() } else { format!("{}:{}@", enc(&p.user), enc(&p.password)) };
            let url = format!("http://{login}{host}:{}", p.port);
            let bypass = p.bypass.split_whitespace().collect::<Vec<_>>().join(",");
            let mut v: Vec<_> = PROXY_VARS.iter().map(|k| (*k, Some(url.clone()))).collect();
            v.push(("NO_PROXY", Some(bypass.clone())));
            v.push(("no_proxy", Some(bypass)));
            v
        }
        // the system's: the programs find it themselves
        _ => Vec::new(),
    }
}

/// The proxy of Options, as the last client was made with it.
pub fn current_proxy() -> ProxyConfig {
    TOOL_PROXY.lock_ok().clone()
}

/// `cmd` started with the proxy of Options.
pub fn with_proxy(cmd: &mut std::process::Command) {
    for (k, v) in proxy_env() {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
}

fn build_client_family(proxy: &ProxyConfig, ipv4_only: bool) -> anyhow::Result<Client> {
    if !ipv4_only {
        *TOOL_PROXY.lock_ok() = proxy.clone();
    }
    let mut b = Client::builder()
        // Real separate TCP connections. With HTTP/2 all our
        // "connections" would share one TCP connection and be as slow as one.
        .http1_only()
        // "User-Agent:", not "user-agent:": the names written the way browsers
        // and other download programs write them. Cloudflare's protection
        // refuses lowercase names (HTTP 403, "challenge") even with the
        // browser's own cookies and name (samfw.com and many more)
        .http1_title_case_headers()
        .tcp_nodelay(true)
        .connect_timeout(Duration::from_secs(20))
        // a server that takes the connection and then says nothing (no
        // answer, or the data stops): the request ends instead of waiting
        // for ever, and the download tries again by itself
        .read_timeout(ANSWER_TIMEOUT)
        .pool_max_idle_per_host(64)
        // redirects are followed by `SendOrdered::send_ordered`, which puts
        // "Host" first again on every step (see there)
        .redirect(reqwest::redirect::Policy::none());
    if ipv4_only {
        b = b.local_address(Some(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)));
    }
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

/// A small file (a few MB at most) fetched whole, e.g. the signed Firefox
/// add-on of a release.
pub async fn fetch_small(proxy: &ProxyConfig, url: &str) -> anyhow::Result<Vec<u8>> {
    let client = build_client_with(proxy)?;
    let resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, crate::config::DEFAULT_USER_AGENT)
        .timeout(Duration::from_secs(60))
        .send_ordered()
        .await?;
    if !resp.status().is_success() {
        return Err(HttpStatus(resp.status().as_u16()).into());
    }
    Ok(resp.bytes().await?.to_vec())
}

/// How many redirects one request follows.
const MAX_REDIRECTS: usize = 15;

/// Sending a request the way browsers and other download programs send it:
/// "Host" is the first line after the request line. The library puts it
/// last, and Cloudflare's protection refuses such a request (HTTP 403,
/// "challenge"), whatever else it carries (samfw.com and many more). The
/// client follows no redirects by itself, so every step of a redirect is
/// sent here too, with the "Host" of its own address.
pub trait SendOrdered {
    fn send_ordered(self) -> impl std::future::Future<Output = reqwest::Result<reqwest::Response>> + Send;
}

impl SendOrdered for RequestBuilder {
    async fn send_ordered(self) -> reqwest::Result<reqwest::Response> {
        let (client, req) = self.build_split();
        let mut req = req?;
        let mut steps = 0;
        loop {
            host_first(&mut req);
            let again = req.try_clone();
            let resp = client.execute(req).await?;
            let status = resp.status();
            if !status.is_redirection() || steps >= MAX_REDIRECTS {
                return Ok(resp);
            }
            let next = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|l| resp.url().join(l.trim()).ok())
                .filter(|u| matches!(u.scheme(), "http" | "https"));
            let (Some(next), Some(mut n)) = (next, again) else { return Ok(resp) };
            // 303, and 301 / 302 after a POST: the next step is a plain GET
            if status == StatusCode::SEE_OTHER
                || (matches!(status, StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND) && n.method() == reqwest::Method::POST)
            {
                *n.method_mut() = reqwest::Method::GET;
                *n.body_mut() = None;
                n.headers_mut().remove(CONTENT_TYPE);
                n.headers_mut().remove(reqwest::header::CONTENT_LENGTH);
            }
            // another site never gets this site's sign-in
            let prev = resp.url();
            // nor does a plain (http) address, where anyone on the way could read it
            let downgraded = prev.scheme() == "https" && next.scheme() != "https";
            if downgraded || next.host_str() != prev.host_str() || next.port_or_known_default() != prev.port_or_known_default() {
                for h in [reqwest::header::AUTHORIZATION, COOKIE, reqwest::header::PROXY_AUTHORIZATION, reqwest::header::WWW_AUTHENTICATE] {
                    n.headers_mut().remove(h);
                }
            }
            *n.url_mut() = next;
            req = n;
            steps += 1;
        }
    }
}

/// "Host" (with the port when it is not the usual one) as the first header.
fn host_first(req: &mut reqwest::Request) {
    let url = req.url();
    let Some(host) = url.host_str() else { return };
    let value = match url.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    };
    let Ok(value) = reqwest::header::HeaderValue::from_str(&value) else { return };
    let old = std::mem::take(req.headers_mut());
    let mut h = reqwest::header::HeaderMap::with_capacity(old.len() + 1);
    h.insert(reqwest::header::HOST, value);
    for (k, v) in old.iter() {
        if k != reqwest::header::HOST {
            h.append(k.clone(), v.clone());
        }
    }
    *req.headers_mut() = h;
}

/// GET request carrying the browser's headers.
pub fn request(client: &Client, url: &str, h: &RequestHeaders, default_ua: &str) -> RequestBuilder {
    let ua = h
        .user_agent
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(default_ua);
    // a form's download is asked the way the form asks: POST with its data
    let form = h.post.as_deref().and_then(crate::checksum::base64);
    let rb = match form {
        Some(body) => client
            .post(url)
            .header(CONTENT_TYPE, h.post_type.as_deref().filter(|t| !t.is_empty()).unwrap_or("application/x-www-form-urlencoded"))
            .body(body),
        None => client.get(url),
    };
    // identity: ranges must refer to the raw bytes, never a compressed body
    let mut rb = rb
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
/// Some file sites refuse a fresh link for a few seconds (HTTP 403 / 429:
/// the link is made ready after a wait on their page): the program asked
/// once and showed the error, a moment later it worked. It is asked again
/// a few times (2, 3, 5, 8 s) before the refusal is believed.
const NOT_READY_WAITS: [u64; 4] = [2, 3, 5, 8];

pub async fn probe(
    client: &Client,
    url: &str,
    headers: &RequestHeaders,
    default_ua: &str,
) -> anyhow::Result<ProbeResult> {
    probe_patient(client, url, headers, default_ua, &NOT_READY_WAITS.map(Duration::from_secs)).await
}

/// One look only (the add-in asks while the browser waits): a refusal is
/// told at once.
pub async fn probe_quick(client: &Client, url: &str, headers: &RequestHeaders, default_ua: &str) -> anyhow::Result<ProbeResult> {
    probe_patient(client, url, headers, default_ua, &[]).await
}

async fn probe_patient(
    client: &Client,
    url: &str,
    headers: &RequestHeaders,
    default_ua: &str,
    waits: &[Duration],
) -> anyhow::Result<ProbeResult> {
    // asked a moment ago and its answer kept: not asked again (a link that
    // works only once is not spent by looking at it)
    if let Some(look) = kept_look(url) {
        return Ok(look);
    }
    let mut waits = waits.iter();
    loop {
        let client = &client_for(client, url);
        let answer = match probe_once(client, url, headers, default_ua).await {
            // refused: over IPv4 (a link for the IP the page saw)
            Err(e) if matches!(http_status_of(&e), Some(403)) && !wants_captcha(&e) => match (host_of(url), v4_client()) {
                (Some(host), Some(v4)) if !V4_HOSTS.lock_ok().contains(&host) => match probe_once(&v4, url, headers, default_ua).await {
                    Ok(p) => {
                        V4_HOSTS.lock_ok().insert(host);
                        Ok(p)
                    }
                    Err(_) => Err(e),
                },
                _ => Err(e),
            },
            other => other,
        };
        match answer {
            Err(e) if matches!(http_status_of(&e), Some(403 | 429)) && !wants_captcha(&e) => match waits.next() {
                Some(w) => tokio::time::sleep(*w).await,
                None => return Err(e),
            },
            other => return other,
        }
    }
}

// ---------------------------------------------------------------- answers kept for the download

/// The first answer about a file is kept open (its data not read yet) for a
/// while: the download goes on from it instead of asking again. A link that
/// works only once (or for one request) was spent by the look of "Download
/// File Info", and the download itself got "403" or a web page.
pub const KEEP_FOR: Duration = Duration::from_secs(600);
/// At most this many answers are kept at once (each holds a connection).
const KEEP_MAX: usize = 6;

struct Kept {
    url: String,
    at: std::time::Instant,
    look: ProbeResult,
    answer: reqwest::Response,
}

static KEPT: std::sync::LazyLock<Mutex<Vec<Kept>>> = std::sync::LazyLock::new(Default::default);

fn kept_list() -> std::sync::MutexGuard<'static, Vec<Kept>> {
    let mut k = KEPT.lock_ok();
    k.retain(|x| x.at.elapsed() < KEEP_FOR);
    k
}

fn keep(url: &str, look: &ProbeResult, answer: reqwest::Response) {
    let mut k = kept_list();
    k.retain(|x| x.url != url);
    k.push(Kept { url: url.to_string(), at: std::time::Instant::now(), look: look.clone(), answer });
    while k.len() > KEEP_MAX {
        k.remove(0);
    }
    drop(k);
    // its connection is let go when nobody took it in time
    if let Ok(rt) = tokio::runtime::Handle::try_current() {
        let url = url.to_string();
        rt.spawn(async move {
            tokio::time::sleep(KEEP_FOR + Duration::from_secs(1)).await;
            drop(kept_list());
            let _ = url;
        });
    }
}

/// What the kept answer for `url` said (it stays kept).
fn kept_look(url: &str) -> Option<ProbeResult> {
    kept_list().iter().find(|x| x.url == url).map(|x| x.look.clone())
}

/// The kept answer for `url`, taken: the download reads the file from it.
pub fn take_kept(url: &str) -> Option<(ProbeResult, reqwest::Response)> {
    let mut k = kept_list();
    let i = k.iter().position(|x| x.url == url)?;
    let x = k.remove(i);
    Some((x.look, x.answer))
}

/// Nobody will take the kept answer for `url` (the window was closed, the
/// download goes on from parts already there): its connection goes.
pub fn drop_kept(url: &str) {
    kept_list().retain(|x| x.url != url);
}

// ---------------------------------------------------------------- pages that lead on to the file

/// Most pages in a row a look follows ("your download starts in 5 seconds").
const MAX_PAGE_HOPS: usize = 5;
/// The longest wait such a page asks for that is kept.
const PAGE_WAIT: Duration = Duration::from_secs(10);
/// How much of a web page is read to find where it leads.
const PAGE_PEEK: usize = 256 * 1024;

/// A site's "not a robot" check turned the request away (it says so in a
/// header of its answer).
pub fn is_bot_check(resp: &reqwest::Response) -> bool {
    resp.status() == StatusCode::FORBIDDEN
        && resp
            .headers()
            .get("cf-mitigated")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("challenge"))
}

/// Where a "Refresh" value leads and after how long: "5; url=next.html",
/// "0;URL='https://...'", "3, https://...". None for a page that only
/// reloads itself, or for anything but a web address.
pub fn refresh_target(value: &str, base: &url::Url) -> Option<(Duration, url::Url)> {
    let (delay, rest) = value.split_once([';', ','])?;
    let delay: f64 = delay.trim().parse().ok().filter(|d: &f64| d.is_finite() && *d >= 0.0)?;
    let rest = rest.trim();
    let target = match rest.get(..3) {
        Some(u) if u.eq_ignore_ascii_case("url") && rest[3..].trim_start().starts_with('=') => rest[3..].trim_start()[1..].trim(),
        _ => rest,
    };
    let target = target.trim_matches(|c| c == '"' || c == '\'').trim();
    let target = target.replace("&amp;", "&");
    let next = base.join(&target).ok()?;
    let mut same = next.clone();
    same.set_fragment(base.fragment());
    (matches!(next.scheme(), "http" | "https") && same != *base).then(|| (Duration::from_secs_f64(delay.min(PAGE_WAIT.as_secs_f64())), next))
}

/// The value of attribute `name` in one tag's text (`<meta a="b" ...>`).
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        // a whole attribute name, not the end of another one
        if at > 0 && !lower.as_bytes()[at - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = tag[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        return Some(value.to_string());
    }
    None
}

/// Where a page's `<meta http-equiv="refresh" content="...">` leads.
pub fn meta_refresh(page: &str, base: &url::Url) -> Option<(Duration, url::Url)> {
    let lower = page.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<meta") {
        let start = from + i;
        let end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &page[start..end];
        from = end;
        let refresh = attribute(tag, "http-equiv").is_some_and(|v| v.trim().eq_ignore_ascii_case("refresh"));
        if refresh {
            if let Some(found) = attribute(tag, "content").and_then(|c| refresh_target(&c, base)) {
                return Some(found);
            }
        }
    }
    None
}

/// The "name=value" pairs a response's Set-Cookie headers set.
fn set_cookies(h: &reqwest::header::HeaderMap) -> Vec<String> {
    h.get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next())
        .map(str::trim)
        .filter(|c| c.contains('='))
        .map(str::to_string)
        .collect()
}

/// `cookies` added to `old` (each replacing one of the same name).
pub fn add_cookies(old: Option<&str>, cookies: &[String]) -> Option<String> {
    if cookies.is_empty() {
        return old.map(str::to_string);
    }
    let name = |c: &str| c.split('=').next().unwrap_or("").trim().to_string();
    let new: std::collections::HashSet<String> = cookies.iter().map(|c| name(c)).collect();
    let mut all: Vec<String> = old
        .unwrap_or("")
        .split(';')
        .map(str::trim)
        .filter(|c| !c.is_empty() && !new.contains(&name(c)))
        .map(str::to_string)
        .collect();
    all.extend(cookies.iter().cloned());
    Some(all.join("; "))
}

/// One look, following the pages that lead on to the file by themselves.
async fn probe_once(client: &Client, url: &str, headers: &RequestHeaders, default_ua: &str) -> anyhow::Result<ProbeResult> {
    let mut at = url.to_string();
    let mut h = headers.clone();
    let mut led = false;
    for hop in 0..=MAX_PAGE_HOPS {
        let (mut look, answer, next) = probe_at(client, &at, &h, default_ua, hop < MAX_PAGE_HOPS).await?;
        if led {
            look.headers_used = Some(look.headers_used.take().unwrap_or_else(|| h.clone()));
        }
        let Some((wait, next, cookies)) = next else {
            if let Some(answer) = answer {
                keep(url, &look, answer);
            }
            return Ok(look);
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        if let Some(used) = look.headers_used.take() {
            h = used;
        }
        let from = url::Url::parse(&at).ok();
        let same_site = from.as_ref().is_some_and(|f| {
            crate::http_download::site_of(f.as_str()) == crate::http_download::site_of(next.as_str())
                && !(f.scheme() == "https" && next.scheme() != "https")
        });
        if same_site {
            h.cookies = add_cookies(h.cookies.as_deref(), &cookies);
        }
        if let Some(f) = from {
            h.referer = Some(f.to_string());
        }
        at = next.to_string();
        led = true;
    }
    bail!("the link opens one web page after another, not a file")
}

/// Where a page leads (wait, address, cookies it set).
type PageNext = (Duration, url::Url, Vec<String>);

async fn probe_at(
    client: &Client,
    url: &str,
    headers: &RequestHeaders,
    default_ua: &str,
    follow_pages: bool,
) -> anyhow::Result<(ProbeResult, Option<reqwest::Response>, Option<PageNext>)> {
    let mut first_refusal: Option<u16> = None;
    let mut found = None;
    for (with_range, h) in fallbacks(headers) {
        let mut rb = request(client, url, &h, default_ua);
        if with_range {
            // from the start to the end, like a browser asking to resume:
            // some servers refuse a tiny range ("bytes=0-0") but take this.
            rb = rb.header(RANGE, "bytes=0-");
        }
        let resp = match tokio::time::timeout(ANSWER_TIMEOUT, rb.send_ordered()).await {
            Ok(r) => r.context("could not connect to server")?,
            Err(_) => return Err(NetDrop("the server did not answer (timed out)").into()),
        };
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED {
            bail!("the server asks for a user name and password (HTTP 401) - use \"Use authorization\" or Options > Site Logins");
        }
        // the site's "not a robot" check: no other way of asking helps
        if is_bot_check(&resp) {
            return Err(anyhow::Error::new(HttpStatus(403)).context(CAPTCHA));
        }
        // refused (forbidden, range not allowed, too many requests...): try
        // the next way; anything else is the server's real answer
        if matches!(status.as_u16(), 403 | 406 | 416 | 429) {
            first_refusal.get_or_insert(status.as_u16());
            continue;
        }
        if !status.is_success() {
            return Err(HttpStatus(status.as_u16()).into());
        }
        // a form's answer that sent it on elsewhere: there the file is asked
        // for plainly (as the browser does after "303 See Other")
        let mut h = h;
        if h.post.is_some() && resp.url().as_str() != url {
            h.post = None;
            h.post_type = None;
        }
        let changed = h != *headers;
        found = Some((resp, with_range, changed.then_some(h)));
        break;
    }
    let Some((mut resp, with_range, headers_used)) = found else {
        return Err(HttpStatus(first_refusal.unwrap_or(403)).into());
    };
    let status = resp.status();
    let final_url = resp.url().to_string();
    let h = resp.headers().clone();
    let content_type = h.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let last_modified = h
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| chrono::DateTime::parse_from_rfc2822(s.trim()).ok())
        .map(|d| d.with_timezone(&chrono::Utc));
    // a strong ETag only ("W/..." may stay the same for another file)
    let etag = h
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.starts_with("W/"));

    let (total_size, resumable, from_zero) = if status == StatusCode::PARTIAL_CONTENT {
        // a real range answer echoes what we asked for: "bytes 0-0/123456".
        // anything else (no header at all, or "bytes */12345") never proved
        // that the server honors ranges
        match h
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range)
        {
            Some(cr) => (cr.total, cr.total.is_some(), cr.first == 0),
            None => (None, false, false),
        }
    } else {
        // the whole file came, not a part: whatever "Accept-Ranges" says,
        // this server did not take a byte range, so one connection and no
        // resume (parts it would not honour were made again and again)
        (resp.content_length(), false, true)
    };

    let named = h.get(CONTENT_DISPOSITION).is_some();
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
    // a server that refused a byte range gets one plain connection
    let resumable = resumable && with_range;
    let look = ProbeResult {
        headers_used,
        final_url,
        file_name,
        total_size: if kind == DownloadKind::Hls { None } else { total_size },
        resumable: kind == DownloadKind::Http && resumable,
        content_type,
        kind,
        last_modified,
        etag,
        digest: crate::checksum::Expected::from_headers(&h, status == StatusCode::OK).map(|e| e.text()),
    };
    // a small web page where a file was expected: a page that leads on to
    // the file by itself ("your download starts in a moment")?
    let small = resp.content_length().is_none_or(|n| n <= PAGE_PEEK as u64);
    {
        if follow_pages && look.is_web_page() && !named && small {
            let base = resp.url().clone();
            let refresh = h.get(reqwest::header::REFRESH).and_then(|v| v.to_str().ok()).and_then(|v| refresh_target(v, &base));
            let next = match refresh {
                Some(n) => Some(n),
                None => {
                    let mut page = Vec::new();
                    while page.len() < PAGE_PEEK {
                        match tokio::time::timeout(Duration::from_secs(10), resp.chunk()).await {
                            Ok(Ok(Some(c))) => page.extend_from_slice(&c),
                            _ => break,
                        }
                    }
                    meta_refresh(&String::from_utf8_lossy(&page), &base)
                }
            };
            if let Some((wait, next)) = next {
                return Ok((look, None, Some((wait, next, set_cookies(&h)))));
            }
            return Ok((look, None, None));
        }
    }
    // kept for the download: a file (not a page, not a stream list) whose
    // answer starts at its first byte
    let keepable = kind == DownloadKind::Http && from_zero && !look.is_web_page();
    Ok((look, keepable.then_some(resp), None))
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

    /// A link for the IP the page saw (IPv4): the server refuses the same
    /// computer over IPv6 and gives the file over IPv4.
    #[tokio::test]
    async fn a_link_for_the_ipv4_address_is_asked_over_ipv4() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let Ok(listener) = tokio::net::TcpListener::bind("[::]:0").await else { return };
        let port = listener.local_addr().unwrap().port();
        // no IPv6 here, or "localhost" is IPv4 only: nothing to see
        if tokio::net::TcpStream::connect(("::1", port)).await.is_err() {
            return;
        }
        tokio::spawn(async move {
            loop {
                let (mut s, from) = listener.accept().await.unwrap();
                let v4 = match from.ip() {
                    std::net::IpAddr::V4(_) => true,
                    std::net::IpAddr::V6(a) => a.to_ipv4_mapped().is_some(),
                };
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let answer = if v4 {
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-9/10\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789"
                } else {
                    "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                };
                let _ = s.write_all(answer.as_bytes()).await;
            }
        });
        let client = build_client_with(&ProxyConfig::default()).unwrap();
        let url = format!("http://[::1]:{port}/v.mp4");
        // over IPv6 the server refuses; the program asks again over IPv4...
        // ([::1] has no IPv4 address: it is refused, as it should be)
        assert!(probe_patient(&client, &url, &RequestHeaders::default(), "UA", &[]).await.is_err());
        let url = format!("http://localhost:{port}/v.mp4");
        let p = probe_patient(&client, &url, &RequestHeaders::default(), "UA", &[]).await.unwrap();
        assert_eq!(p.total_size, Some(10));
        // and the download asks that server over IPv4 too
        assert!(V4_HOSTS.lock_ok().contains("localhost") || p.total_size == Some(10));
    }

    /// A site that refuses a fresh link for a moment (403), then gives it.
    #[tokio::test]
    async fn a_link_not_ready_yet_is_asked_again() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let opened = std::time::Instant::now();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let answer = if opened.elapsed() < Duration::from_millis(700) {
                    "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-9/10\r\nContent-Length: 10\r\nContent-Type: video/mp4\r\nConnection: close\r\n\r\n0123456789".to_string()
                };
                let _ = s.write_all(answer.as_bytes()).await;
            }
        });
        let client = build_client().unwrap();
        let url = format!("http://127.0.0.1:{port}/v.mp4");
        let waits = [Duration::from_millis(300); 4];
        let p = probe_patient(&client, &url, &RequestHeaders::default(), "UA", &waits).await.unwrap();
        assert_eq!(p.total_size, Some(10));
        // refused for good: the 403 after the tries
        let e = probe_patient(&client, "http://127.0.0.1:1/x", &RequestHeaders::default(), "UA", &waits).await;
        assert!(e.is_err());
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

    /// A server like Cloudflare's protection: lowercase header names are a
    /// program, not a browser, and get HTTP 403.
    #[tokio::test]
    async fn header_names_are_written_like_a_browser() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let browser_like = ["User-Agent: ", "Accept: ", "Range: ", "Referer: ", "Cookie: "].iter().all(|h| req.contains(h))
                        && !req.contains("user-agent:")
                        && req.lines().nth(1).is_some_and(|l| l.starts_with("Host: 127.0.0.1:"));
                    let resp = if browser_like {
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 0-4/5\r\nConnection: close\r\n\r\nhello"
                    } else {
                        "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    };
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        let client = build_client_with(&ProxyConfig { mode: 0, ..Default::default() }).unwrap();
        let headers = RequestHeaders { cookies: Some("a=1".into()), referer: Some("https://example.com/".into()), ..Default::default() };
        let p = probe(&client, &format!("http://{addr}/f.zip"), &headers, "Mozilla/5.0 test").await.unwrap();
        assert_eq!(p.total_size, Some(5));
        assert!(p.headers_used.is_none(), "the first, full request was taken");
    }

    /// A redirect to another site: "Host" first and right on every step,
    /// and the first site's cookie stays with the first site.
    #[tokio::test]
    async fn redirects_to_another_site_keep_host_first() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let second = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port2 = second.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = second.accept().await {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let ok = req.lines().nth(1) == Some(&format!("Host: localhost:{port2}")) && !req.contains("Cookie:");
                let resp = if ok {
                    "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello".to_string()
                } else {
                    "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                };
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });
        let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = first.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = first.accept().await {
                let mut buf = vec![0u8; 8192];
                let _ = s.read(&mut buf).await;
                let resp = format!("HTTP/1.1 302 Found\r\nLocation: http://localhost:{port2}/real.zip\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });
        let client = build_client_with(&ProxyConfig { mode: 0, ..Default::default() }).unwrap();
        let headers = RequestHeaders { cookies: Some("only=first".into()), ..Default::default() };
        let p = probe(&client, &format!("http://{addr}/go"), &headers, "Mozilla/5.0 test").await.unwrap();
        assert_eq!(p.total_size, Some(5));
        assert!(p.final_url.ends_with("/real.zip"));
    }

    /// How the test server answers the probe's range request.
    enum ProbeMode {
        /// a well-formed 206 echo: "bytes 0-0/12345"
        Valid206,
        /// plain 200 that only advertises range support
        Full200,
        /// 206 whose "Content-Range: bytes */12345" names no range
        Stars206,
        /// 206 with a junk "Content-Range"
        Garbage206,
    }

    /// Answers every request with the headers chosen by `mode`; the body is
    /// always one piece so a 200 probe can still read a size.
    async fn probe_server(mode: ProbeMode) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let head = match mode {
            ProbeMode::Valid206 => (
                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/12345\r\nContent-Length: 1\r\n",
                "h",
            ),
            ProbeMode::Full200 => (
                "HTTP/1.1 200 OK\r\nAccept-Ranges: bytes\r\nContent-Length: 5\r\n",
                "hello",
            ),
            ProbeMode::Stars206 => (
                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes */12345\r\nContent-Length: 1\r\n",
                "h",
            ),
            ProbeMode::Garbage206 => (
                "HTTP/1.1 206 Partial Content\r\nContent-Range: not-a-range\r\nContent-Length: 1\r\n",
                "h",
            ),
        };
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let head = head;
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let _ = s.read(&mut buf).await.unwrap_or(0);
                    let resp = format!(
                        "{}Content-Type: application/octet-stream\r\nConnection: close\r\n\r\n{}",
                        head.0, head.1
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}/tool.bin")
    }

    #[tokio::test]
    async fn probe_resumable_on_valid_206() {
        let url = probe_server(ProbeMode::Valid206).await;
        let client = Client::builder().no_proxy().build().unwrap();
        let p = probe(&client, &url, &RequestHeaders::default(), "UA").await.unwrap();
        assert!(p.resumable, "a well-formed range echo proves range support");
        assert_eq!(p.total_size, Some(12345));
    }

    #[tokio::test]
    async fn probe_not_resumable_when_range_ignored() {
        let url = probe_server(ProbeMode::Full200).await;
        let client = Client::builder().no_proxy().build().unwrap();
        let p = probe(&client, &url, &RequestHeaders::default(), "UA").await.unwrap();
        assert!(!p.resumable, "the server ignored the range: one plain connection");
        assert_eq!(p.total_size, Some(5), "the size is still known from Content-Length");
    }

    #[tokio::test]
    async fn probe_not_resumable_on_malformed_206() {
        let client = Client::builder().no_proxy().build().unwrap();
        for mode in [ProbeMode::Stars206, ProbeMode::Garbage206] {
            let url = probe_server(mode).await;
            let p = probe(&client, &url, &RequestHeaders::default(), "UA").await.unwrap();
            assert!(!p.resumable, "a 206 without a real range echo proves nothing");
            assert_eq!(p.total_size, None, "no usable Content-Range: the size is unknown");
        }
    }

    #[test]
    fn content_range_parsing() {
        let cr = parse_content_range("bytes 0-0/123456").unwrap();
        assert_eq!(cr, ContentRange { first: 0, last: 0, total: Some(123456) });
        let cr = parse_content_range("bytes 100-199/*").unwrap();
        assert_eq!(cr.total, None, "the open-ended form has no total");
        assert!(parse_content_range("bytes */1000").is_none(), "the 416 shape names no range");
        assert!(parse_content_range("bytes 5-2/100").is_none(), "backwards range");
        assert!(parse_content_range("not-a-range").is_none());
    }
}

#[cfg(test)]
mod passing_tests {
    use super::*;
    #[test]
    fn helper_programs_get_the_proxy() {
        let manual = ProxyConfig { mode: 2, host: "http://10.0.0.1".into(), port: 8080, user: "a b".into(), password: "p@ss".into(), bypass: "local.test  x.test".into() };
        let env = env_for(&manual);
        let get = |k: &str| env.iter().find(|(n, _)| *n == k).and_then(|(_, v)| v.clone());
        assert_eq!(get("HTTPS_PROXY").as_deref(), Some("http://a%20b:p%40ss@10.0.0.1:8080"));
        assert_eq!(get("NO_PROXY").as_deref(), Some("local.test,x.test"));
        let none = env_for(&ProxyConfig { mode: 0, ..Default::default() });
        assert!(none.iter().any(|(k, v)| *k == "http_proxy" && v.is_none()));
        assert!(none.iter().any(|(k, v)| *k == "NO_PROXY" && v.as_deref() == Some("*")));
        assert!(env_for(&ProxyConfig { mode: 1, ..Default::default() }).is_empty());
    }

    #[test]
    fn which_failures_pass() {
        assert!(is_passing_error(&anyhow::Error::new(HttpStatus(503))));
        assert!(is_passing_error(&anyhow::Error::new(HttpStatus(429))));
        assert!(!is_passing_error(&anyhow::Error::new(HttpStatus(404))));
        assert!(!is_passing_error(&anyhow::Error::new(HttpStatus(403))));
        assert!(is_passing_error(&anyhow::anyhow!("download ended before all parts were received")));
        assert!(!is_passing_error(&anyhow::anyhow!("not enough room on the disk")));
        assert!(!is_passing_error(&anyhow::Error::new(HttpStatus(410)).context("the link has expired or the file was removed")));
        // soon at first, never more than a minute
        assert_eq!(retry_delay(1).as_secs(), 3);
        assert_eq!(retry_delay(20).as_secs(), 60);
    }
}
