//! HLS (.m3u8) video streams: download every part with several connections,
//! decrypt AES-128 parts when needed, then join them into one video file.
//!
//! DRM protected streams (SAMPLE-AES, Widevine, FairPlay...) are refused on
//! purpose. Live streams (no #EXT-X-ENDLIST) are refused too.

use crate::http::{send, ConnGuard, Progress, Shared};
use crate::http_download::Cancelled;
use crate::model::RequestHeaders;
use anyhow::{anyhow, bail, Context};
use std::collections::HashMap;
use crate::m3u8::{best_variant, parse_media};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

const MAX_SEGMENT_BYTES: usize = 64 << 20;
const MAX_INIT_BYTES: usize = 16 << 20;
const MAX_KEYS: usize = 4096;
const READ_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct HlsJob {
    pub url: String,
    pub headers: RequestHeaders,
    pub user_agent: String,
    /// final output file (written as .sgpart first by the caller)
    pub output: PathBuf,
    pub temp_dir: PathBuf,
    pub connections: u32,
    pub retries: u32,
    pub limiter: Arc<crate::limiter::SpeedLimiter>,
}

fn decrypt(key: &[u8], iv: &[u8; 16], data: &mut Vec<u8>) -> anyhow::Result<()> {
    use aes::Aes128;
    use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    let dec = cbc::Decryptor::<Aes128>::new_from_slices(key, iv)
        .map_err(|_| anyhow!("invalid AES key length"))?;
    let len = dec
        .decrypt_padded_mut::<Pkcs7>(data)
        .map_err(|_| anyhow!("could not decrypt stream part"))?
        .len();
    data.truncate(len);
    Ok(())
}

/// Bound each network read before allocating. Plain segments stream to disk;
/// only playlists, tiny keys and AES-encrypted segments need a memory buffer.
async fn fetch_body(
    shared: &Shared,
    job: &HlsJob,
    url: &str,
    max_bytes: usize,
    cancel: &CancellationToken,
    destination: Option<&Path>,
) -> anyhow::Result<(Vec<u8>, u64)> {
    let client = shared.client();
    let mut resp = tokio::select! {
        _ = cancel.cancelled() => return Err(Cancelled.into()),
        r = tokio::time::timeout(READ_IDLE_TIMEOUT, send(&client, url, &job.headers, &job.user_agent, None)) => {
            r.context("stream request timed out")??
        }
    };
    if !resp.status().is_success() {
        bail!("HTTP {} for {}", resp.status().as_u16(), url);
    }
    if resp.content_length().is_some_and(|n| n > max_bytes as u64) {
        bail!("stream response exceeds the {max_bytes}-byte safety limit");
    }
    let mut file = match destination {
        Some(path) => Some(tokio::fs::File::create(path).await?),
        None => None,
    };
    let mut data = Vec::new();
    let mut received = 0usize;
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err(Cancelled.into()),
            r = tokio::time::timeout(READ_IDLE_TIMEOUT, resp.chunk()) => r.context("stream read timed out")??,
        };
        let Some(chunk) = chunk else { break };
        received = received.checked_add(chunk.len()).filter(|n| *n <= max_bytes)
            .ok_or_else(|| anyhow!("stream response exceeds the {max_bytes}-byte safety limit"))?;
        tokio::select! {
            _ = cancel.cancelled() => return Err(Cancelled.into()),
            _ = async { shared.limiter.acquire(chunk.len()).await; job.limiter.acquire(chunk.len()).await; } => {}
        }
        if let Some(file) = file.as_mut() {
            file.write_all(&chunk).await?;
        } else {
            data.extend_from_slice(&chunk);
        }
    }
    if let Some(file) = file.as_mut() {
        file.flush().await?;
    }
    Ok((data, received as u64))
}

async fn fetch_bytes(shared: &Shared, job: &HlsJob, url: &str, max_bytes: usize, cancel: &CancellationToken) -> anyhow::Result<Vec<u8>> {
    Ok(fetch_body(shared, job, url, max_bytes, cancel, None).await?.0)
}

async fn fetch_text(shared: &Shared, job: &HlsJob, url: &str, cancel: &CancellationToken) -> anyhow::Result<String> {
    let b = fetch_bytes(shared, job, url, crate::m3u8::MAX_PLAYLIST_BYTES, cancel).await?;
    String::from_utf8(b).context("stream playlist is not UTF-8")
}

fn part_file(dir: &Path, i: usize) -> PathBuf {
    dir.join(format!("{i:06}.part"))
}

struct PartTemp(PathBuf);

impl Drop for PartTemp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub async fn run(
    shared: Arc<Shared>,
    mut job: HlsJob,
    progress: Arc<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    job.headers.bind_to(&job.url);
    let job = Arc::new(job);
    // 1. playlist (follow a master playlist to its best quality)
    let mut url = job.url.clone();
    let mut text = fetch_text(&shared, &job, &url, &cancel).await.context("could not load playlist")?;
    if !text.trim_start().starts_with("#EXTM3U") {
        bail!("not a valid HLS playlist");
    }
    if text.contains("#EXT-X-STREAM-INF") {
        url = best_variant(&url, &text)?.ok_or_else(|| anyhow!("no video variant found"))?;
        text = fetch_text(&shared, &job, &url, &cancel).await?;
    }
    let playlist = parse_media(&url, &text)?;
    if !playlist.ended {
        bail!("live streams are not supported (the playlist has no end)");
    }
    if playlist.parts.is_empty() {
        bail!("the playlist has no parts");
    }
    std::fs::create_dir_all(&job.temp_dir)?;

    // 2. keys (usually one for the whole stream)
    let mut keys: HashMap<String, Vec<u8>> = HashMap::new();
    for p in &playlist.parts {
        if let Some(k) = &p.key {
            if !keys.contains_key(&k.uri) {
                if keys.len() >= MAX_KEYS {
                    bail!("the stream uses too many encryption keys");
                }
                let bytes = fetch_bytes(&shared, &job, &k.uri, 16, &cancel).await.context("could not load key")?;
                if bytes.len() != 16 {
                    bail!("an AES-128 stream key must contain exactly 16 bytes");
                }
                keys.insert(k.uri.clone(), bytes);
            }
        }
    }
    let keys = Arc::new(keys);
    let parts = Arc::new(playlist.parts);

    // 3. parts, several at a time; finished parts survive pause/resume
    let total = parts.len();
    progress.parts_total.store(total as u32, Ordering::Relaxed);
    let mut done_bytes = 0u64;
    let mut done = 0u32;
    for i in 0..total {
        if let Ok(m) = std::fs::metadata(part_file(&job.temp_dir, i)) {
            done += 1;
            done_bytes += m.len();
        }
    }
    progress.parts_done.store(done, Ordering::Relaxed);
    progress.bytes.store(done_bytes, Ordering::Relaxed);

    let next = Arc::new(AtomicUsize::new(0));
    let child = cancel.child_token();
    let mut set = JoinSet::new();
    // Four encrypted buffers have a hard aggregate ceiling of 256 MiB;
    // ordinary segments are streamed rather than buffered at all.
    for _ in 0..job.connections.clamp(1, 4) {
        let (shared, job, progress, child, next, parts, keys) = (
            shared.clone(),
            job.clone(),
            progress.clone(),
            child.clone(),
            next.clone(),
            parts.clone(),
            keys.clone(),
        );
        set.spawn(async move {
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= parts.len() {
                    return Ok::<(), anyhow::Error>(());
                }
                let path = part_file(&job.temp_dir, i);
                if path.exists() {
                    continue;
                }
                let mut attempt = 0;
                loop {
                    // Keep cleanup outside the selected future: cancelling a
                    // stream drops that future before its normal error path.
                    let temp = PartTemp(path.with_extension("tmp"));
                    let fut = async {
                        let _g = ConnGuard::new(&progress);
                        let tmp = &temp.0;
                        let result = async {
                          let size = if let Some(k) = &parts[i].key {
                            let mut data = fetch_bytes(&shared, &job, &parts[i].url, MAX_SEGMENT_BYTES, &child).await?;
                            let key = keys.get(&k.uri).ok_or_else(|| anyhow!("missing key"))?;
                            let iv = k
                                .iv
                                .unwrap_or_else(|| (parts[i].sequence as u128).to_be_bytes());
                            decrypt(key, &iv, &mut data)?;
                            tokio::fs::write(&tmp, &data).await?;
                            data.len() as u64
                          } else {
                            fetch_body(&shared, &job, &parts[i].url, MAX_SEGMENT_BYTES, &child, Some(&tmp)).await?.1
                          };
                          tokio::fs::rename(&tmp, &path).await?;
                          Ok::<u64, anyhow::Error>(size)
                        }.await;
                        if result.is_err() {
                            let _ = tokio::fs::remove_file(&tmp).await;
                        }
                        result
                    };
                    let r = tokio::select! {
                        r = fut => r,
                        _ = child.cancelled() => return Err(Cancelled.into()),
                    };
                    match r {
                        Ok(n) => {
                            progress.bytes.fetch_add(n, Ordering::Relaxed);
                            progress.parts_done.fetch_add(1, Ordering::Relaxed);
                            break;
                        }
                        Err(e) => {
                            if e.chain().any(|cause| cause.is::<Cancelled>()) {
                                return Err(e);
                            }
                            attempt += 1;
                            if attempt > job.retries {
                                return Err(e);
                            }
                            tokio::select! {
                                _ = child.cancelled() => return Err(Cancelled.into()),
                                _ = tokio::time::sleep(Duration::from_secs((attempt as u64 * 2).min(20))) => {}
                            }
                        }
                    }
                }
            }
        });
    }
    let mut first_error = None;
    while let Some(r) = set.join_next().await {
        let r = r.map_err(|e| anyhow!("worker crashed: {e}")).and_then(|r| r);
        if let Err(e) = r {
            if first_error.is_none() && !cancel.is_cancelled() {
                first_error = Some(e);
            }
            child.cancel();
        }
    }
    if cancel.is_cancelled() {
        return Err(Cancelled.into());
    }
    if let Some(e) = first_error {
        return Err(e);
    }

    // 4. join everything into one file
    progress.assembling.store(true, Ordering::Relaxed);
    let init = match &playlist.init_url {
        Some(u) => Some(fetch_bytes(&shared, &job, u, MAX_INIT_BYTES, &cancel).await?),
        None => None,
    };
    let temp_dir = job.temp_dir.clone();
    let output = job.output.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&output)?);
        if let Some(init) = init {
            out.write_all(&init)?;
        }
        for i in 0..total {
            let mut f = std::fs::File::open(part_file(&temp_dir, i))?;
            std::io::copy(&mut f, &mut out)?;
        }
        out.flush()?;
        drop(out);
        let _ = std::fs::remove_dir_all(&temp_dir);
        Ok(())
    })
    .await??;
    progress.assembling.store(false, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::build_client;
    use crate::limiter::SpeedLimiter;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn shared() -> Arc<Shared> {
        Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)))
    }

    fn job(url: String, dir: &Path) -> HlsJob {
        HlsJob {
            url,
            headers: RequestHeaders::default(),
            user_agent: "local-hls-test".into(),
            output: dir.join("out.ts"),
            temp_dir: dir.join("parts"),
            connections: 4,
            retries: 0,
            limiter: Arc::new(SpeedLimiter::new(0)),
        }
    }

    async fn response(bytes: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/stream", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).await;
            let _ = socket.write_all(&bytes).await;
        });
        (url, task)
    }

    #[tokio::test]
    async fn rejects_declared_and_chunked_oversized_bodies() {
        for bytes in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 1000000000\r\nConnection: close\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n8\r\nabcdefgh\r\n0\r\n\r\n".to_vec(),
        ] {
            let (url, task) = response(bytes).await;
            let j = job(url.clone(), Path::new("/unused"));
            let error = fetch_bytes(&shared(), &j, &url, 12, &CancellationToken::new()).await.unwrap_err();
            assert!(error.to_string().contains("safety limit"), "{error:#}");
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn cancellation_interrupts_a_stalled_initial_playlist() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/stream.m3u8", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).await;
            std::future::pending::<()>().await;
        });
        let cancel = CancellationToken::new();
        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            c.cancel();
        });
        let j = job(url, Path::new("/unused"));
        let error = tokio::time::timeout(Duration::from_secs(1), run(shared(), j, Arc::new(Progress::default()), cancel))
            .await.expect("pause must interrupt the initial network wait").unwrap_err();
        assert!(error.chain().any(|e| e.is::<Cancelled>()));
        task.abort();
    }

    async fn stream_server(key: Option<Vec<u8>>, stall_part: bool) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/stream.m3u8", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let key = key.clone();
                tokio::spawn(async move {
                    let mut request = [0u8; 4096];
                    let n = socket.read(&mut request).await.unwrap_or(0);
                    let path = std::str::from_utf8(&request[..n]).unwrap_or("").split_whitespace().nth(1).unwrap_or("");
                    let body = match path {
                        "/stream.m3u8" => {
                            let encryption = if key.is_some() { "#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n" } else { "" };
                            format!("#EXTM3U\n#EXT-X-MAP:URI=\"init\"\n{encryption}#EXTINF:1,\npart1\n#EXTINF:1,\npart2\n#EXT-X-ENDLIST\n").into_bytes()
                        }
                        "/key" => key.unwrap_or_default(),
                        "/init" => b"init-".to_vec(),
                        "/part1" if stall_part => vec![0; 4096],
                        "/part1" => b"first-".to_vec(),
                        "/part2" => b"second".to_vec(),
                        _ => Vec::new(),
                    };
                    let stalled = stall_part && path == "/part1";
                    let length = if stalled { body.len() + 1_000_000 } else { body.len() };
                    let headers = format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n");
                    let _ = socket.write_all(headers.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                    if stalled { std::future::pending::<()>().await; }
                });
            }
        });
        (url, task)
    }

    fn test_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ezoo-hls-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn streams_plain_parts_to_disk_and_joins_in_order() {
        let (url, server) = stream_server(None, false).await;
        let dir = test_dir("plain");
        let j = job(url, &dir);
        let progress = Arc::new(Progress::default());
        run(shared(), j.clone(), progress.clone(), CancellationToken::new()).await.unwrap();
        assert_eq!(std::fs::read(&j.output).unwrap(), b"init-first-second");
        assert_eq!(progress.parts_done.load(Ordering::Relaxed), 2);
        assert!(!j.temp_dir.exists());
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn rejects_wrong_length_aes_key_before_downloading_parts() {
        let (url, server) = stream_server(Some(vec![0; 15]), false).await;
        let dir = test_dir("key");
        let j = job(url, &dir);
        let error = run(shared(), j.clone(), Arc::new(Progress::default()), CancellationToken::new()).await.unwrap_err();
        assert!(error.to_string().contains("exactly 16 bytes"), "{error:#}");
        assert!(!j.output.exists());
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn pausing_a_stream_removes_its_partial_segment() {
        let (url, server) = stream_server(None, true).await;
        let dir = test_dir("pause");
        let j = job(url, &dir);
        let partial = j.temp_dir.join("000000.tmp");
        let cancel = CancellationToken::new();
        let task = tokio::spawn(run(shared(), j.clone(), Arc::new(Progress::default()), cancel.clone()));
        let mut started = false;
        for _ in 0..100 {
            if std::fs::metadata(&partial).is_ok_and(|m| m.len() > 0) {
                started = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(started, "test server should deliver a partial segment");
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), task).await.unwrap().unwrap();
        assert!(result.unwrap_err().chain().any(|e| e.is::<Cancelled>()));
        assert!(!partial.exists());
        assert!(!j.output.exists());
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
