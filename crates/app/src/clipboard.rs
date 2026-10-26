//! Clipboard monitor: when the user copies a link to a downloadable file,
//! offer to download it (like classic download managers).

use std::time::Duration;

pub const DEFAULT_EXTENSIONS: &[&str] = &[
    "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "iso", "exe", "msi", "msix", "apk", "dmg",
    "deb", "rpm", "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mp3", "wav", "flac",
    "aac", "m4a", "ogg", "pdf", "epub", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "m3u8", "bin",
    "img", "torrent",
];

/// Returns the URL when `text` is a single http(s) link to a known file type using default extensions.
pub fn downloadable_url(text: &str) -> Option<String> {
    downloadable_url_with(text, None)
}

/// Returns the URL when `text` is a single http(s) link to a known file type.
/// If `custom_exts` is provided and non-empty, checks against them (wildcards like "r0*" supported).
pub fn downloadable_url_with(text: &str, custom_exts: Option<&[String]>) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.len() > 4096 || t.contains(char::is_whitespace) {
        return None;
    }
    // a torrent's magnet link
    if swiftget_engine::torrent::is_magnet(t) {
        return Some(t.to_string());
    }
    let u = url::Url::parse(t).ok()?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    let path = u.path().to_ascii_lowercase();
    let ext = path.rsplit('.').next()?;
    if !path.contains('.') {
        return None;
    }

    let is_match = match custom_exts {
        Some(custom) if !custom.is_empty() => custom.iter().any(|c| {
            let pattern = c.trim().to_ascii_lowercase();
            if pattern.contains('*') || pattern.contains('?') {
                swiftget_engine::config::wildcard_match(&pattern, ext)
            } else {
                pattern == ext
            }
        }),
        _ => DEFAULT_EXTENSIONS.contains(&ext),
    };

    if is_match {
        Some(t.to_string())
    } else {
        None
    }
}

/// Polls the clipboard once a second on its own thread.
pub fn spawn(
    enabled: impl Fn() -> bool + Send + 'static,
    get_exts: impl Fn() -> Vec<String> + Send + 'static,
    on_url: impl Fn(String) + Send + 'static,
) {
    std::thread::Builder::new()
        .name("swiftget-clipboard".into())
        .spawn(move || {
            let Ok(mut clip) = arboard::Clipboard::new() else { return };
            // ignore whatever was already copied before we started
            let mut last = clip.get_text().unwrap_or_default();
            loop {
                std::thread::sleep(Duration::from_millis(1000));
                let Ok(text) = clip.get_text() else { continue };
                if text == last {
                    continue;
                }
                last = text.clone();
                if !enabled() {
                    continue;
                }
                let exts = get_exts();
                if let Some(u) = downloadable_url_with(&text, Some(&exts)) {
                    on_url(u);
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downloadable_url_default() {
        assert_eq!(
            downloadable_url("https://example.com/file.zip"),
            Some("https://example.com/file.zip".into())
        );
        assert_eq!(
            downloadable_url("https://example.com/video.mp4"),
            Some("https://example.com/video.mp4".into())
        );
        assert_eq!(downloadable_url("https://example.com/page.html"), None);
        assert_eq!(downloadable_url("not a url"), None);
    }

    #[test]
    fn test_downloadable_url_with_custom_exts() {
        let custom = vec!["custom".to_string(), "r0*".to_string()];
        assert_eq!(
            downloadable_url_with("https://example.com/archive.custom", Some(&custom)),
            Some("https://example.com/archive.custom".into())
        );
        assert_eq!(
            downloadable_url_with("https://example.com/archive.r01", Some(&custom)),
            Some("https://example.com/archive.r01".into())
        );
        assert_eq!(
            downloadable_url_with("https://example.com/archive.zip", Some(&custom)),
            None
        );
    }

    #[test]
    fn test_downloadable_url_magnet() {
        let magnet = "magnet:?xt=urn:btih:da39a3ee5e6b4b0d3255bfef95601890afd80709";
        assert_eq!(downloadable_url(magnet), Some(magnet.into()));
    }
}
