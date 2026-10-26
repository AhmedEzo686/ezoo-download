//! Synthetic loopback origins exercise credential boundaries without real logins.

use std::sync::{Arc, Mutex};
use swiftget_engine::grabber::{crawl, GrabOptions, GrabProgress};
use swiftget_engine::http::{build_client_with, probe, send, Progress, ProxyConfig, Shared};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::{hls, RequestHeaders};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

struct Server {
    origin: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) { self.task.abort(); }
}

async fn server(other: Option<String>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requests: Arc<Mutex<Vec<String>>> = Arc::default();
    let captured = requests.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else { break };
            let captured = captured.clone();
            let other = other.clone();
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut read = BufReader::new(read);
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    if read.read_line(&mut line).await.unwrap_or(0) == 0 { return; }
                    request.push_str(&line);
                    if line == "\r\n" { break; }
                }
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                captured.lock().unwrap().push(request);
                let (status, content_type, extra, body) = match (path.as_str(), other.as_deref()) {
                    ("/redirect", Some(other)) => (302, "text/plain", format!("Location: {other}/file.bin\r\n"), String::new()),
                    ("/same-redirect", _) => (302, "text/plain", "Location: /file.bin\r\n".into(), String::new()),
                    ("/crawl", Some(other)) => (200, "text/html", String::new(), format!("<a href='{other}/foreign.html'>next</a><a href='/file.bin'>file</a>")),
                    ("/foreign.html", _) => (200, "text/html", String::new(), "<html></html>".into()),
                    ("/stream.m3u8", Some(other)) => (200, "application/vnd.apple.mpegurl", String::new(), format!("#EXTM3U\n#EXT-X-MAP:URI=\"{other}/init.bin\"\n#EXTINF:1,\n/local.ts\n#EXTINF:1,\n{other}/foreign.ts\n#EXT-X-ENDLIST\n")),
                    _ => (200, "application/octet-stream", String::new(), "data".into()),
                };
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = write.write_all(response.as_bytes()).await;
            });
        }
    });
    Server { origin, requests, task }
}

fn client() -> reqwest::Client {
    build_client_with(&ProxyConfig { mode: 0, ..Default::default() }).unwrap()
}

fn credentials(origin: &str) -> RequestHeaders {
    let mut headers = RequestHeaders {
        cookies: Some("session=synthetic-cookie".into()),
        auth: Some(("synthetic-user".into(), "synthetic-password".into())),
        extra: vec![("X-Api-Key".into(), "synthetic-extra".into())],
        referer: Some(format!("{origin}/private?token=synthetic-referrer")),
        ..Default::default()
    };
    headers.bind_to(origin);
    headers
}

fn has_credentials(request: &str) -> bool {
    let request = request.to_ascii_lowercase();
    request.contains("\r\ncookie:") || request.contains("\r\nauthorization:") || request.contains("\r\nx-api-key:")
}

#[tokio::test]
async fn redirect_hops_and_final_downloads_preserve_origin_boundaries() {
    let foreign = server(None).await;
    let own = server(Some(foreign.origin.clone())).await;
    let client = client();
    let headers = credentials(&own.origin);
    let p = probe(&client, &format!("{}/redirect", own.origin), &headers, "test").await.unwrap();
    assert_eq!(p.final_url, format!("{}/file.bin", foreign.origin));
    // This is the manager's second request after probing a final URL.
    send(&client, &p.final_url, &headers, "test", None).await.unwrap();
    send(&client, &format!("{}/same-redirect", own.origin), &headers, "test", None).await.unwrap();
    let foreign_requests = foreign.requests.lock().unwrap();
    assert!(foreign_requests.len() >= 2);
    for request in foreign_requests.iter() {
        assert!(!has_credentials(request), "{request}");
        assert!(!request.contains("synthetic-referrer"), "{request}");
    }
    let own_requests = own.requests.lock().unwrap();
    assert!(own_requests.iter().all(|request| has_credentials(request)));
    assert!(own_requests.iter().any(|request| request.starts_with("GET /file.bin ")));
}

#[tokio::test]
async fn foreign_crawler_pages_do_not_inherit_the_starting_sites_login() {
    let foreign = server(None).await;
    let own = server(Some(foreign.origin.clone())).await;
    let files = crawl(client(), GrabOptions {
        start_url: format!("{}/crawl", own.origin), depth: 1, other_sites: true, no_parent: false,
        exts: Vec::new(), include: Vec::new(), exclude: Vec::new(), max_pages: 10, parallel: 2,
        headers: credentials(&own.origin), user_agent: "test".into(),
    }, Arc::new(GrabProgress::default()), CancellationToken::new(), |_| {}).await;
    assert!(files.iter().any(|file| file.url.ends_with("/file.bin")));
    let requests = foreign.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(!has_credentials(&requests[0]));
    assert!(!requests[0].contains("synthetic-referrer"));
    assert!(has_credentials(&own.requests.lock().unwrap()[0]));
}

#[tokio::test]
async fn hls_initialization_and_segments_are_filtered_per_destination() {
    let foreign = server(None).await;
    let own = server(Some(foreign.origin.clone())).await;
    let dir = std::env::temp_dir().join(format!("ezoo-credential-hls-{}-{}", std::process::id(), own.origin.rsplit(':').next().unwrap()));
    std::fs::create_dir_all(&dir).unwrap();
    let shared = Arc::new(Shared::new(client(), SpeedLimiter::new(0)));
    hls::run(shared, hls::HlsJob {
        url: format!("{}/stream.m3u8", own.origin), headers: credentials(&own.origin), user_agent: "test".into(),
        output: dir.join("out.ts"), temp_dir: dir.join("parts"), connections: 2, retries: 0,
        limiter: Arc::new(SpeedLimiter::new(0)),
    }, Arc::new(Progress::default()), CancellationToken::new()).await.unwrap();
    assert_eq!(std::fs::read(dir.join("out.ts")).unwrap(), b"datadatadata");
    let requests = foreign.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| !has_credentials(request) && !request.contains("synthetic-referrer")));
    assert!(own.requests.lock().unwrap().iter().all(|request| has_credentials(request)));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_headers_bind_once_and_credentials_never_cross_scheme_or_port() {
    let mut legacy: RequestHeaders = serde_json::from_str(r#"{"referer":null,"cookies":"session=synthetic","user_agent":null,"auth":["u","p"],"extra":[["X-Api-Key","synthetic"]]}"#).unwrap();
    assert!(legacy.credential_origin.is_none());
    legacy.bind_to("https://example.com/private");
    legacy.bind_to("https://other.example/");
    assert_eq!(legacy.credential_origin.as_deref(), Some("https://example.com"));
    assert!(legacy.scoped_for("https://example.com:443/file").auth.is_some());
    for destination in ["http://example.com/file", "https://example.com:8443/file", "https://other.example/file"] {
        let safe = legacy.scoped_for(destination);
        assert!(safe.auth.is_none() && safe.cookies.is_none() && safe.extra.is_empty(), "{destination}");
    }
    let mut headers = credentials("https://example.com");
    headers.referer = Some("https://u:p@example.com/private?token=synthetic#fragment".into());
    assert_eq!(headers.scoped_for("https://other.example/file").referer.as_deref(), Some("https://example.com/"));
    assert!(headers.scoped_for("http://other.example/file").referer.is_none());
}
