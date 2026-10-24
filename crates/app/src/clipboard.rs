//! Clipboard monitor: when the user copies a link to a downloadable file,
//! offer to download it (like classic download managers).

use std::time::Duration;

const EXTENSIONS: &[&str] = &[
    "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "iso", "exe", "msi", "msix", "apk", "dmg",
    "deb", "rpm", "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mp3", "wav", "flac",
    "aac", "m4a", "ogg", "pdf", "epub", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "m3u8", "bin",
    "img", "torrent",
];

/// Returns the URL when `text` is a single http(s) link to a known file type.
pub fn downloadable_url(text: &str) -> Option<String> {
    downloadable_url_with_exts(text, &[])
}

/// Checks against built-in extensions plus any user-configured capture extensions.
pub fn downloadable_url_with_exts(text: &str, custom_exts: &[String]) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.len() > 4096 || t.contains(char::is_whitespace) {
        return None;
    }
    let u = url::Url::parse(t).ok()?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    let path = u.path().to_ascii_lowercase();
    let ext = path.rsplit('.').next()?;
    if path.contains('.')
        && (EXTENSIONS.contains(&ext) || custom_exts.iter().any(|e| e.eq_ignore_ascii_case(ext)))
    {
        Some(t.to_string())
    } else {
        None
    }
}

/// Polls the clipboard once a second on its own thread.
pub fn spawn(
    enabled: impl Fn() -> bool + Send + 'static,
    custom_exts: impl Fn() -> Vec<String> + Send + 'static,
    on_url: impl Fn(String) + Send + 'static,
) {
    std::thread::Builder::new()
        .name("ezoo-clipboard".into())
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
                let exts = custom_exts();
                if let Some(u) = downloadable_url_with_exts(&text, &exts) {
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
    fn test_downloadable_url() {
        assert_eq!(
            downloadable_url("https://example.com/file.zip"),
            Some("https://example.com/file.zip".into())
        );
        assert_eq!(
            downloadable_url("https://example.com/archive.tar.gz"),
            Some("https://example.com/archive.tar.gz".into())
        );
        assert_eq!(downloadable_url("not a url"), None);
        assert_eq!(downloadable_url("ftp://example.com/file.zip"), None);
        assert_eq!(downloadable_url("https://example.com/index.html"), None);

        let custom = vec!["customext".to_string(), "xyz".to_string()];
        assert_eq!(
            downloadable_url_with_exts("https://example.com/file.customext", &custom),
            Some("https://example.com/file.customext".into())
        );
        assert_eq!(
            downloadable_url_with_exts("https://example.com/file.XYZ", &custom),
            Some("https://example.com/file.XYZ".into())
        );
    }
}
