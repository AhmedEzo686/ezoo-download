//! Event sounds (Options > Sounds): plays a .wav file without waiting.

use std::path::Path;

pub fn play(path: &str) {
    let path = path.trim().trim_matches('"');
    if path.is_empty() || !Path::new(path).exists() {
        return;
    }
    imp::play(path);
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};

    pub fn play(path: &str) {
        let wide: Vec<u16> = std::ffi::OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect();
        unsafe {
            PlaySoundW(wide.as_ptr(), std::ptr::null_mut(), SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn play(_path: &str) {}
}
