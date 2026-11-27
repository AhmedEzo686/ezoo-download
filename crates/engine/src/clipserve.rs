//! "Download a part of a video": ffmpeg reads the video from here, on this
//! computer only (127.0.0.1, an address nobody can guess), and this asks
//! the site for exactly the bytes ffmpeg wants, with the download's own
//! headers / sign-in. ffmpeg jumps to the part asked for and reads only
//! that, so the rest of the video is never downloaded.
//!
//! The site is asked in pieces of a few MB, the next one already on its
//! way while the last one is handed over: big video sites send a piece at
//! a time quickly, but slow down a single endless request.

use crate::sync::LockOk;
use crate::http::SendOrdered;
use crate::http::{request, Progress, Shared};
use crate::model::RequestHeaders;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Bytes asked of the site at a time: a small first piece (ffmpeg often
/// reads a little and jumps elsewhere), then bigger ones while it goes on.
const FIRST_PIECE: u64 = 256 << 10;
const PIECE: u64 = 1 << 20;

/// What one address of ours stands for.
#[derive(Clone)]
pub struct Source {
    pub url: String,
    pub headers: RequestHeaders,
    pub user_agent: String,
}

struct Entry {
    src: Source,
    shared: Arc<Shared>,
    /// what came counts for this download's speed and size
    progress: Arc<Progress>,
}

type Table = Arc<Mutex<HashMap<String, Arc<Entry>>>>;

static SERVER: std::sync::OnceLock<(u16, Table)> = std::sync::OnceLock::new();

/// An address of ours for one stream; it stops working when this goes.
pub struct Ticket {
    pub url: String,
    token: String,
    table: Table,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.table.lock_ok().remove(&self.token);
    }
}

/// The server runs on a thread of its own for as long as the program does
/// (not tied to whoever asked first).
fn server() -> anyhow::Result<&'static (u16, Table)> {
    if let Some(s) = SERVER.get() {
        return Ok(s);
    }
    let std_listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    std_listener.set_nonblocking(true)?;
    let port = std_listener.local_addr()?.port();
    let table: Table = Arc::new(Mutex::new(HashMap::new()));
    let t = table.clone();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("ezoo-clip").enable_all().build()?;
    std::thread::Builder::new().name("ezoo-clip-server".into()).spawn(move || {
        rt.block_on(async move {
            let Ok(listener) = TcpListener::from_std(std_listener) else { return };
            loop {
                let Ok((sock, peer)) = listener.accept().await else { continue };
                // this computer only
                if !peer.ip().is_loopback() {
                    continue;
                }
                // what ffmpeg does not take stays at the site: when it
                // jumps elsewhere, little was fetched for nothing
                let _ = socket2::SockRef::from(&sock).set_send_buffer_size(128 << 10);
                let t = t.clone();
                tokio::spawn(async move {
                    let _ = serve(sock, t).await;
                });
            }
        });
    })?;
    // two asking at once: the first one's server is the one
    let _ = SERVER.set((port, table));
    Ok(SERVER.get().expect("set above"))
}

/// Our address for `src` (as long as the ticket lives).
pub async fn open(shared: Arc<Shared>, src: Source, progress: Arc<Progress>) -> anyhow::Result<Ticket> {
    let (port, table) = server()?;
    let token = crate::secret::random_token();
    table.lock_ok().insert(token.clone(), Arc::new(Entry { src, shared, progress }));
    Ok(Ticket { url: format!("http://127.0.0.1:{port}/c/{token}"), token, table: table.clone() })
}

/// The bytes asked for: "bytes=100-" -> (100, None), "bytes=100-199" -> (100, Some(199)).
fn parse_range(v: &str) -> Option<(u64, Option<u64>)> {
    let r = v.trim().strip_prefix("bytes=")?;
    if r.contains(',') {
        return None;
    }
    let (a, b) = r.split_once('-')?;
    let a: u64 = a.trim().parse().ok()?;
    let b = b.trim();
    let b = if b.is_empty() { None } else { Some(b.parse::<u64>().ok()?) };
    if b.is_some_and(|b| b < a) {
        return None;
    }
    Some((a, b))
}

/// "bytes 0-99/1000" -> 1000 (None when the size is not given: "/*").
fn total_of(content_range: &str) -> Option<u64> {
    content_range.rsplit_once('/')?.1.trim().parse().ok()
}

async fn reply_status(sock: &mut TcpStream, code: &str, extra: &str) -> anyhow::Result<()> {
    let head = format!("HTTP/1.1 {code}\r\nContent-Length: 0\r\n{extra}Connection: close\r\n\r\n");
    sock.write_all(head.as_bytes()).await?;
    Ok(())
}

async fn serve(mut sock: TcpStream, table: Table) -> anyhow::Result<()> {
    // the request (ffmpeg's are small)
    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 2048];
    let head_end = loop {
        let n = tokio::time::timeout(std::time::Duration::from_secs(20), sock.read(&mut tmp)).await??;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 16 << 10 {
            return reply_status(&mut sock, "431 Too Large", "").await;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut words = first.split_whitespace();
    let (method, path) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    let only_head = method == "HEAD";
    if method != "GET" && !only_head {
        return reply_status(&mut sock, "405 Method Not Allowed", "").await;
    }
    let token = path.strip_prefix("/c/").unwrap_or("");
    let Some(entry) = table.lock_ok().get(token).cloned() else {
        return reply_status(&mut sock, "404 Not Found", "").await;
    };
    let mut asked: Option<(u64, Option<u64>)> = None;
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            if k.trim().eq_ignore_ascii_case("range") {
                asked = parse_range(v);
            }
        }
    }
    let (from, until) = asked.unwrap_or((0, None));

    // the first piece: tells how big the file is
    let client = entry.shared.client();
    let piece_end = |at: u64, size: u64| until.map(|u| u.min(at + size - 1)).unwrap_or(at + size - 1);
    let ask = |a: u64, b: Option<u64>| {
        let range = match b {
            Some(b) => format!("bytes={a}-{b}"),
            None => format!("bytes={a}-"),
        };
        request(&client, &entry.src.url, &entry.src.headers, &entry.src.user_agent)
            .header(reqwest::header::RANGE, range)
            .send_ordered()
    };
    let mut resp = match ask(from, Some(piece_end(from, FIRST_PIECE))).await {
        Ok(r) => r,
        Err(_) => return reply_status(&mut sock, "502 Bad Gateway", "").await,
    };
    let status = resp.status();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        let cr = resp.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let extra = if cr.is_empty() { String::new() } else { format!("Content-Range: {cr}\r\n") };
        return reply_status(&mut sock, "416 Range Not Satisfiable", &extra).await;
    }
    if !status.is_success() {
        let code = format!("{} Refused", status.as_u16());
        return reply_status(&mut sock, &code, "").await;
    }
    let partial = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let total = if partial {
        resp.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok()).and_then(total_of)
    } else {
        resp.content_length()
    };
    let Some(total) = total.filter(|t| *t > 0) else {
        // size unknown: handed over as it comes, without pieces
        if partial {
            resp = match ask(from, until).await {
                Ok(r) if r.status().is_success() => r,
                _ => return reply_status(&mut sock, "502 Bad Gateway", "").await,
            };
        }
        let head = format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n");
        sock.write_all(head.as_bytes()).await?;
        if !only_head {
            pass_on(&mut sock, &mut resp, &entry, 0, u64::MAX).await?;
        }
        return Ok(());
    };
    if from >= total {
        return reply_status(&mut sock, "416 Range Not Satisfiable", &format!("Content-Range: bytes */{total}\r\n")).await;
    }
    let last = until.unwrap_or(total - 1).min(total - 1);
    let head = if asked.is_some() {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Type: {content_type}\r\nContent-Range: bytes {from}-{last}/{total}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
            last - from + 1
        )
    } else {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {total}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
        )
    };
    sock.write_all(head.as_bytes()).await?;
    if only_head {
        return Ok(());
    }
    if !partial {
        // the site sends the whole file every time: skip to `from`
        return pass_on(&mut sock, &mut resp, &entry, from, last - from + 1).await;
    }
    // piece after piece, each one bigger; once ffmpeg reads on, the next
    // one is asked for while this one is passed on
    let mut at = from;
    let mut size = FIRST_PIECE;
    let mut current = Some(resp);
    let mut pieces = 0;
    while at <= last {
        let end = piece_end(at, size).min(last);
        let next_at = end + 1;
        let next_size = (size * 2).min(PIECE);
        pieces += 1;
        let next = (next_at <= last && pieces >= 3).then(|| tokio::spawn(ask(next_at, Some(piece_end(next_at, next_size).min(last)))));
        let mut r = match current.take() {
            Some(r) => r,
            None => break,
        };
        let mut tries = 0;
        loop {
            let want = end - at + 1;
            match pass_piece(&mut sock, &mut r, &entry, want).await {
                Ok(got) if got == want => break,
                Ok(got) => {
                    // the site stopped early: the rest of the piece again
                    at += got;
                    tries += 1;
                    if tries > 3 {
                        return Ok(());
                    }
                    r = match ask(at, Some(end)).await {
                        Ok(x) if x.status() == reqwest::StatusCode::PARTIAL_CONTENT => x,
                        _ => return Ok(()),
                    };
                }
                // ffmpeg went on elsewhere (closed): nothing more to send
                Err(_) => return Ok(()),
            }
        }
        at = next_at;
        size = next_size;
        if at > last {
            break;
        }
        let fresh = match next {
            Some(n) => match n.await {
                Ok(Ok(x)) if x.status() == reqwest::StatusCode::PARTIAL_CONTENT => Some(x),
                _ => None,
            },
            None => None,
        };
        current = match fresh {
            Some(x) => Some(x),
            None => match ask(at, Some(piece_end(at, size).min(last))).await {
                Ok(x) if x.status() == reqwest::StatusCode::PARTIAL_CONTENT => Some(x),
                _ => return Ok(()),
            },
        };
    }
    Ok(())
}

/// Up to `want` bytes of `resp` to ffmpeg; how many went.
async fn pass_piece(sock: &mut TcpStream, resp: &mut reqwest::Response, entry: &Entry, want: u64) -> anyhow::Result<u64> {
    let mut sent = 0u64;
    while sent < want {
        let Ok(Some(chunk)) = resp.chunk().await else { break };
        let take = chunk.len().min((want - sent) as usize);
        count(entry, take).await;
        sock.write_all(&chunk[..take]).await?;
        sent += take as u64;
    }
    Ok(sent)
}

/// A whole answer to ffmpeg, without its first `skip` bytes, at most `want`.
async fn pass_on(sock: &mut TcpStream, resp: &mut reqwest::Response, entry: &Entry, mut skip: u64, want: u64) -> anyhow::Result<()> {
    let mut sent = 0u64;
    while sent < want {
        let Ok(Some(chunk)) = resp.chunk().await else { break };
        let mut data = &chunk[..];
        if skip > 0 {
            let s = skip.min(data.len() as u64) as usize;
            data = &data[s..];
            skip -= s as u64;
        }
        let take = data.len().min((want - sent).min(usize::MAX as u64) as usize);
        if take == 0 {
            continue;
        }
        count(entry, take).await;
        sock.write_all(&data[..take]).await?;
        sent += take as u64;
    }
    Ok(())
}

async fn count(entry: &Entry, n: usize) {
    entry.shared.limiter.acquire(n).await;
    entry.progress.bytes.fetch_add(n as u64, Ordering::Relaxed);
    entry.progress.online();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=100-"), Some((100, None)));
        assert_eq!(parse_range(" bytes=0-99"), Some((0, Some(99))));
        assert_eq!(parse_range("bytes=9-3"), None);
        assert_eq!(parse_range("bytes=0-1,5-9"), None);
        assert_eq!(parse_range("items=0-1"), None);
        assert_eq!(total_of("bytes 0-99/1000"), Some(1000));
        assert_eq!(total_of("bytes 0-99/*"), None);
    }
}
