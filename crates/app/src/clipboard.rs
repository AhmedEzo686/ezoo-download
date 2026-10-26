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
    if path.contains('.') && EXTENSIONS.contains(&ext) {
        Some(t.to_string())
    } else {
        None
    }
}

/// Owns clipboard access only while monitoring is enabled. The first successful
/// read after enabling is a baseline, never a download request.
#[derive(Default)]
struct Monitor<C> {
    clipboard: Option<C>,
    last: Option<String>,
}

impl<C> Monitor<C> {
    fn poll(
        &mut self,
        enabled: bool,
        create: impl FnOnce() -> Option<C>,
        read: impl FnOnce(&mut C) -> Option<String>,
    ) -> Option<String> {
        if !enabled {
            self.clipboard = None;
            self.last = None;
            return None;
        }
        if self.clipboard.is_none() {
            self.clipboard = create();
        }
        let text = read(self.clipboard.as_mut()?)?;
        let previous = self.last.replace(text.clone())?;
        if text == previous {
            return None;
        }
        downloadable_url(&text)
    }
}

/// Polls once a second; disabling monitoring releases the clipboard and does
/// not initialize or read it until monitoring is enabled again.
pub fn spawn(enabled: impl Fn() -> bool + Send + 'static, on_url: impl Fn(String) + Send + 'static) {
    std::thread::Builder::new()
        .name("swiftget-clipboard".into())
        .spawn(move || {
            let mut monitor = Monitor { clipboard: None, last: None };
            loop {
                if let Some(u) = monitor.poll(enabled(), || arboard::Clipboard::new().ok(), |clip| clip.get_text().ok()) {
                    on_url(u);
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn disabled_monitor_never_initializes_or_reads() {
        let mut monitor = Monitor::<()>::default();
        assert_eq!(monitor.poll(false, || panic!("clipboard initialized while disabled"), |_| panic!("clipboard read while disabled")), None);
        assert!(monitor.clipboard.is_none());
        assert!(monitor.last.is_none());
    }

    #[test]
    fn each_activation_rebases_before_reporting_new_links() {
        let created = Cell::new(0);
        let mut monitor = Monitor::<()>::default();
        let mut poll = |enabled, text: &str| monitor.poll(enabled, || {
            created.set(created.get() + 1);
            Some(())
        }, |_| Some(text.to_owned()));
        assert_eq!(poll(true, "https://example.com/old.zip"), None);
        assert_eq!(poll(true, "https://example.com/old.zip"), None);
        assert_eq!(poll(true, "https://example.com/new.zip"), Some("https://example.com/new.zip".into()));
        assert_eq!(poll(false, "https://example.com/private.zip"), None);
        assert_eq!(poll(true, "https://example.com/private.zip"), None);
        assert_eq!(poll(true, "https://example.com/later.zip"), Some("https://example.com/later.zip".into()));
        assert_eq!(created.get(), 2);
    }

    #[test]
    fn failed_reads_do_not_consume_the_initial_baseline() {
        let mut monitor = Monitor::<()>::default();
        assert_eq!(monitor.poll(true, || None, |_| panic!("read without clipboard")), None);
        assert_eq!(monitor.poll(true, || Some(()), |_| None), None);
        assert_eq!(monitor.poll(true, || panic!("already initialized"), |_| Some("https://example.com/baseline.zip".into())), None);
        assert_eq!(monitor.poll(true, || panic!("already initialized"), |_| Some("https://example.com/changed.zip".into())), Some("https://example.com/changed.zip".into()));
    }
}
