//! Clipboard monitor: when the user copies a link to a downloadable file,
//! offer to download it (like classic download managers).
//!
//! On Windows the program doesn't look at the clipboard again and again:
//! Windows itself tells it when something new was copied (the way Windows
//! asks programs to watch it), and only then, with the monitor turned on, is
//! the text read. A program reading the clipboard every second is what
//! antivirus programs look for in the ones that swap copied text.

#[cfg(not(windows))]
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

/// Watches the clipboard on its own thread; `on_url` gets each downloadable
/// link copied while `enabled()` says so.
pub fn spawn(
    enabled: impl Fn() -> bool + Send + 'static,
    own_types: impl Fn() -> Vec<String> + Send + 'static,
    on_url: impl Fn(String) + Send + 'static,
) {
    std::thread::Builder::new()
        .name("ezoo-clipboard".into())
        .spawn(move || {
            let mut last: Option<String> = None;
            let mut copied = move || {
                if !enabled() {
                    return;
                }
                let Ok(mut clip) = arboard::Clipboard::new() else { return };
                let Ok(text) = clip.get_text() else { return };
                if last.as_deref() == Some(text.as_str()) {
                    return;
                }
                last = Some(text.clone());
                if let Some(u) = downloadable_url_with(&text, &own_types()) {
                    on_url(u);
                }
            };
            watch(&mut copied);
        })
        .ok();
}

/// Windows says when the clipboard changed (a window of ours, never shown,
/// is on its list of clipboard listeners); `copied` runs then.
#[cfg(windows)]
fn watch(copied: &mut dyn FnMut()) {
    use windows_sys::Win32::System::DataExchange::{AddClipboardFormatListener, RemoveClipboardFormatListener};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW, RegisterClassW, TranslateMessage,
        HWND_MESSAGE, MSG, WM_APP, WM_CLIPBOARDUPDATE, WNDCLASSW,
    };
    const COPIED: u32 = WM_APP + 1;

    unsafe extern "system" fn proc_(hwnd: windows_sys::Win32::Foundation::HWND, msg: u32, wp: usize, lp: isize) -> isize {
        if msg == WM_CLIPBOARDUPDATE {
            // handled in the loop below (this is called from inside it)
            // SAFETY: our own window
            unsafe { PostMessageW(hwnd, COPIED, 0, 0) };
            return 0;
        }
        // SAFETY: every other message as Windows handles it
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    let class: Vec<u16> = "EzooDownloadClipboard\0".encode_utf16().collect();
    // SAFETY: a window class of this program, its name living through the calls
    unsafe {
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(proc_),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: std::ptr::null_mut(),
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        // a message-only window: never seen, only receives messages
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        if hwnd.is_null() || AddClipboardFormatListener(hwnd) == 0 {
            if !hwnd.is_null() {
                DestroyWindow(hwnd);
            }
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == COPIED && msg.hwnd == hwnd {
                copied();
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        RemoveClipboardFormatListener(hwnd);
        DestroyWindow(hwnd);
    }
}

/// Elsewhere: a look every second, when the monitor is on.
#[cfg(not(windows))]
fn watch(copied: &mut dyn FnMut()) {
    loop {
        std::thread::sleep(Duration::from_millis(1000));
        copied();
    }
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
