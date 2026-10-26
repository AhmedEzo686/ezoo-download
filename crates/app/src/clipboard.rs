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
    downloadable_url_with(text, &[])
}

/// The same, also taking the file types the user added in Options
/// ("File Types" list of the browser add-in).
pub fn downloadable_url_with(text: &str, own: &[String]) -> Option<String> {
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
    if path.contains('.') && (EXTENSIONS.contains(&ext) || own.iter().any(|e| e.eq_ignore_ascii_case(ext))) {
        Some(t.to_string())
    } else {
        None
    }
}

/// Polls the clipboard once a second on its own thread.
pub fn spawn(
    enabled: impl Fn() -> bool + Send + 'static,
    own_types: impl Fn() -> Vec<String> + Send + 'static,
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
                if let Some(u) = downloadable_url_with(&text, &own_types()) {
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
    fn links_in_the_clipboard() {
        assert_eq!(downloadable_url("https://example.com/file.zip").as_deref(), Some("https://example.com/file.zip"));
        assert_eq!(downloadable_url("https://example.com/index.html"), None);
        assert_eq!(downloadable_url("two words"), None);
        let own = vec!["xyz".to_string()];
        assert_eq!(downloadable_url_with("https://example.com/a.XYZ", &own).as_deref(), Some("https://example.com/a.XYZ"));
        assert_eq!(downloadable_url("https://example.com/a.xyz"), None);
    }
}
