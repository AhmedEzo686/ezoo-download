//! Site Grabber: links that name no file ("get.php?id=5") but give one are
//! found, with the server's name for them; a link is asked what it is.

use std::sync::Arc;
use swiftget_engine::grabber::{self, Checked, GrabOptions, GrabProgress};
use swiftget_engine::http::{build_client_with, ProxyConfig};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

async fn site() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((s, _)) = l.accept().await {
            tokio::spawn(async move {
                let (r, mut w) = s.into_split();
                let mut r = BufReader::new(r);
                let mut first = String::new();
                if r.read_line(&mut first).await.unwrap_or(0) == 0 {
                    return;
                }
                loop {
                    let mut l = String::new();
                    if r.read_line(&mut l).await.unwrap_or(0) == 0 || l.trim().is_empty() {
                        break;
                    }
                }
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (ct, extra, body): (&str, &str, Vec<u8>) = match path.as_str() {
                    "/" => ("text/html", "", br#"<a href="/get.php?id=5">Download</a> <a href="/more">more</a> <a href="/about">about</a>"#.to_vec()),
                    "/more" => ("text/html", "", br#"<a href="/dl/77">the second file</a>"#.to_vec()),
                    "/get.php?id=5" => ("application/octet-stream", "Content-Disposition: attachment; filename=\"report 2026.zip\"\r\n", vec![1; 5000]),
                    "/dl/77" => ("application/pdf", "", vec![2; 3000]),
                    "/about" => ("text/html", "", b"<p>hello</p>".to_vec()),
                    _ => {
                        let _ = w.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                        return;
                    }
                };
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = w.write_all(head.as_bytes()).await;
                let _ = w.write_all(&body).await;
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn files_behind_links_with_no_file_name() {
    let port = site().await;
    let client = build_client_with(&ProxyConfig::default()).unwrap();
    let opts = GrabOptions {
        start_url: format!("http://127.0.0.1:{port}/"),
        depth: 0,
        other_sites: false,
        no_parent: false,
        exts: Vec::new(),
        include: Vec::new(),
        exclude: Vec::new(),
        max_pages: 50,
        parallel: 2,
        headers: Default::default(),
        user_agent: "test".into(),
    };
    // the start page only: its "Download" link (no file name) is asked, and is a file
    let found = grabber::crawl(client.clone(), opts.clone(), Arc::new(GrabProgress::default()), CancellationToken::new(), |_| {}).await;
    let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["report 2026.zip"], "{found:?}");
    // one level deeper: the next page's "/dl/77" too
    let found = grabber::crawl(client.clone(), GrabOptions { depth: 1, ..opts }, Arc::new(GrabProgress::default()), CancellationToken::new(), |_| {}).await;
    assert!(found.iter().any(|f| f.url.ends_with("/dl/77") && f.ext == "pdf"), "{found:?}");
    assert_eq!(found.len(), 2, "{found:?}");

    // a link is asked what it is
    let ask = |p: &str| format!("http://127.0.0.1:{port}{p}");
    match grabber::check_link(&client, &ask("/get.php?id=5"), &Default::default(), "test").await {
        Checked::File { name, size } => {
            assert_eq!(name, "report 2026.zip");
            assert_eq!(size, Some(5000));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(grabber::check_link(&client, &ask("/about"), &Default::default(), "test").await, Checked::Page);
    assert!(matches!(grabber::check_link(&client, &ask("/nothing"), &Default::default(), "test").await, Checked::Gone(_)));
}
