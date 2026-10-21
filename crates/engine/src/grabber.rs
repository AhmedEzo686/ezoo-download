//! Site Grabber: explores a web site from a start page and collects the
//! files it links to (images, videos, music, archives, documents...).

use crate::config::wildcard_match;
use crate::filename;
use crate::http::request;
use crate::model::RequestHeaders;
use regex::Regex;
use reqwest::Client;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct GrabOptions {
    pub start_url: String,
    /// levels of pages to explore below the start page (0 = start page only)
    pub depth: u32,
    /// explore pages of other sites too (one level)
    pub other_sites: bool,
    /// don't go above the start page's folder
    pub no_parent: bool,
    /// file types to collect; empty = every file that isn't a web page
    pub exts: Vec<String>,
    /// only addresses matching one of these (* allowed); empty = all
    pub include: Vec<String>,
    /// skip addresses matching any of these
    pub exclude: Vec<String>,
    /// stop after this many pages
    pub max_pages: usize,
    /// pages explored at the same time (1-10)
    pub parallel: usize,
    pub headers: RequestHeaders,
    pub user_agent: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FoundFile {
    pub url: String,
    pub name: String,
    pub ext: String,
    /// the link's text (used as the description)
    pub text: String,
    /// page the link was found on
    pub page: String,
}

/// Live counters for the grabber window.
#[derive(Default)]
pub struct GrabProgress {
    pub pages_done: AtomicUsize,
    pub pages_queued: AtomicUsize,
    pub files: AtomicUsize,
    pub current: Mutex<String>,
}

const PAGE_EXTS: [&str; 13] =
    ["", "html", "htm", "php", "asp", "aspx", "jsp", "shtml", "cfm", "cgi", "pl", "xhtml", "phtml"];

pub fn is_page_ext(ext: &str) -> bool {
    PAGE_EXTS.contains(&ext)
}

fn anchor_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?is)<a\b[^>]*?\bhref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))[^>]*>(.*?)</a\s*>"#).unwrap()
    })
}

fn attr_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?i)\b(?:src|href|data-src|data-href|poster)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap()
    })
}

fn tag_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?s)<[^>]*>").unwrap())
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

/// Every link of an HTML page as (absolute address, link text).
pub fn extract_links(html: &str, base: &str) -> Vec<(String, String)> {
    let Ok(base) = url::Url::parse(base) else { return Vec::new() };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut push = |raw: &str, text: String| {
        let raw = decode_entities(raw.trim());
        if raw.is_empty()
            || raw.starts_with('#')
            || ["javascript:", "mailto:", "data:", "tel:", "about:"]
                .iter()
                .any(|p| raw.to_ascii_lowercase().starts_with(p))
        {
            return;
        }
        let Ok(mut u) = base.join(&raw) else { return };
        if !matches!(u.scheme(), "http" | "https") {
            return;
        }
        u.set_fragment(None);
        let s = u.to_string();
        if seen.insert(s.clone()) {
            out.push((s, text));
        }
    };
    for c in anchor_re().captures_iter(html) {
        let href = c.get(1).or(c.get(2)).or(c.get(3)).map(|m| m.as_str()).unwrap_or("");
        let text = c.get(4).map(|m| tag_re().replace_all(m.as_str(), " ").to_string()).unwrap_or_default();
        let text = decode_entities(&text).split_whitespace().collect::<Vec<_>>().join(" ");
        push(href, text);
    }
    for c in attr_re().captures_iter(html) {
        let v = c.get(1).or(c.get(2)).map(|m| m.as_str()).unwrap_or("");
        push(v, String::new());
    }
    out
}

fn ext_of(u: &url::Url) -> String {
    let last = u.path_segments().and_then(|mut s| s.next_back()).unwrap_or("");
    let last = percent_encoding::percent_decode_str(last).decode_utf8_lossy().to_string();
    filename::extension(&last)
}

fn wanted_ext(opts: &GrabOptions, ext: &str) -> bool {
    if opts.exts.is_empty() {
        return !is_page_ext(ext);
    }
    opts.exts.iter().any(|e| wildcard_match(e.trim_start_matches('.'), ext))
}

fn allowed(opts: &GrabOptions, url: &str) -> bool {
    if opts.exclude.iter().any(|p| !p.is_empty() && wildcard_match(p, url)) {
        return false;
    }
    opts.include.iter().all(|p| p.is_empty()) || opts.include.iter().any(|p| !p.is_empty() && wildcard_match(p, url))
}

async fn fetch_page(client: &Client, url: &str, opts: &GrabOptions) -> Option<(String, String)> {
    let resp = request(client, url, &opts.headers, &opts.user_agent).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !(ct.is_empty() || ct.contains("html") || ct.contains("xml")) {
        return None;
    }
    let final_url = resp.url().to_string();
    // read at most 4 MB of a page
    let mut resp = resp;
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > 4 * 1024 * 1024 {
            break;
        }
    }
    Some((final_url, String::from_utf8_lossy(&body).into_owned()))
}

/// Explore the site and return the files found (in the order found).
pub async fn crawl(
    client: Client,
    opts: GrabOptions,
    progress: Arc<GrabProgress>,
    cancel: CancellationToken,
    on_file: impl Fn(FoundFile) + Send + Sync + 'static,
) -> Vec<FoundFile> {
    let Ok(start) = url::Url::parse(&opts.start_url) else { return Vec::new() };
    let start_host = start.host_str().unwrap_or("").to_ascii_lowercase();
    let start_dir = {
        let p = start.path();
        p[..p.rfind('/').map(|i| i + 1).unwrap_or(p.len())].to_string()
    };
    let on_file = Arc::new(on_file);
    let mut found: Vec<FoundFile> = Vec::new();
    let mut seen_files: HashSet<String> = HashSet::new();
    let mut seen_pages: HashSet<String> = HashSet::new();
    // (url, level, on another site)
    let mut queue: VecDeque<(String, u32, bool)> = VecDeque::new();
    queue.push_back((opts.start_url.clone(), 0, false));
    seen_pages.insert(opts.start_url.clone());
    progress.pages_queued.store(1, Ordering::Relaxed);
    let parallel = opts.parallel.clamp(1, 10);

    while !queue.is_empty() && !cancel.is_cancelled() {
        let mut batch = Vec::new();
        while batch.len() < parallel {
            match queue.pop_front() {
                Some(item) => batch.push(item),
                None => break,
            }
        }
        let mut tasks = tokio::task::JoinSet::new();
        for (url, level, foreign) in batch {
            let client = client.clone();
            let opts2 = opts.clone();
            if let Ok(mut c) = progress.current.lock() {
                *c = url.clone();
            }
            tasks.spawn(async move {
                let page = fetch_page(&client, &url, &opts2).await;
                (url, level, foreign, page)
            });
        }
        while let Some(res) = tokio::select! {
            r = tasks.join_next() => r,
            _ = cancel.cancelled() => None,
        } {
            let Ok((url, level, foreign, page)) = res else { continue };
            progress.pages_done.fetch_add(1, Ordering::Relaxed);
            let Some((page_url, html)) = page else { continue };
            for (link, text) in extract_links(&html, &page_url) {
                let Ok(u) = url::Url::parse(&link) else { continue };
                let host = u.host_str().unwrap_or("").to_ascii_lowercase();
                let ext = ext_of(&u);
                if !allowed(&opts, &link) {
                    continue;
                }
                if is_page_ext(&ext) && (opts.exts.is_empty() || !wanted_ext(&opts, &ext)) {
                    // a page: explore it later if it's within the limits
                    let same = host == start_host;
                    let next_level = level + 1;
                    let ok_site = same || (opts.other_sites && !foreign);
                    let ok_parent = !opts.no_parent || !same || u.path().starts_with(&start_dir);
                    if next_level <= opts.depth
                        && ok_site
                        && ok_parent
                        && seen_pages.len() < opts.max_pages.max(1)
                        && seen_pages.insert(link.clone())
                    {
                        queue.push_back((link, next_level, !same));
                        progress.pages_queued.fetch_add(1, Ordering::Relaxed);
                    }
                    continue;
                }
                if wanted_ext(&opts, &ext) && seen_files.insert(link.clone()) {
                    let name = filename::from_url(&link).unwrap_or_else(|| "download".into());
                    let f = FoundFile { url: link, name, ext, text: text.clone(), page: url.clone() };
                    progress.files.fetch_add(1, Ordering::Relaxed);
                    on_file(f.clone());
                    found.push(f);
                }
            }
        }
    }
    found
}

/// File types of the grabber's ready-made filters.
pub fn template_exts(template: i32) -> Vec<String> {
    let list = match template {
        // all images
        1 => "jpg jpeg png gif webp bmp svg tif tiff ico heic",
        // all videos
        2 => "mp4 mkv avi mov wmv flv webm m4v ts 3gp mpg mpeg",
        // all music
        3 => "mp3 wav flac aac ogg m4a wma opus",
        // archives + programs
        4 => "zip rar 7z tar gz tgz bz2 xz iso exe msi apk dmg",
        // documents
        5 => "pdf doc docx xls xlsx ppt pptx txt epub",
        // every file
        _ => "",
    };
    list.split_whitespace().map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_from_html() {
        let html = r##"<html><body>
            <a href="files/a.pdf">Document <b>A</b></a>
            <a href='/b.pdf?x=1&amp;y=2'>B</a>
            <a href="#top">top</a> <a href="mailto:x@y.z">mail</a>
            <img src="//cdn.example.com/p.png"> <a href=page2.html>next</a>
        </body></html>"##;
        let l = extract_links(html, "https://example.com/dir/index.html");
        let urls: Vec<&str> = l.iter().map(|(u, _)| u.as_str()).collect();
        assert!(urls.contains(&"https://example.com/dir/files/a.pdf"));
        assert!(urls.contains(&"https://example.com/b.pdf?x=1&y=2"));
        assert!(urls.contains(&"https://cdn.example.com/p.png"));
        assert!(urls.contains(&"https://example.com/dir/page2.html"));
        assert!(!urls.iter().any(|u| u.contains("mailto") || u.ends_with("#top")));
        assert_eq!(l.iter().find(|(u, _)| u.ends_with("a.pdf")).unwrap().1, "Document A");
    }
}
