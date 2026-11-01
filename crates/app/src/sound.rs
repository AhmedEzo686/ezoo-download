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

/// Linux: the first command-line player that is there. Each one gets the
/// arguments that make it play a file and say nothing; it runs in the
/// background, so a slow start never holds up the window.
#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;

    /// (program, arguments before the file)
    fn find_player() -> Option<(&'static str, &'static [&'static str])> {
        const CANDIDATES: &[(&str, &[&str])] = &[
            // PulseAudio / PipeWire
            ("paplay", &[]),
            // PipeWire itself
            ("pw-play", &[]),
            // ffmpeg: decodes every .wav, even unusual ones
            ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
            // plain ALSA
            ("aplay", &["-q"]),
            // libcanberra (the desktop's own sound player)
            ("canberra-gtk-play", &["--file"]),
        ];
        static FOUND: OnceLock<Option<usize>> = OnceLock::new();
        let idx = (*FOUND.get_or_init(|| {
            CANDIDATES.iter().position(|(program, _)| {
                let Some(path) = std::env::var_os("PATH") else { return false };
                std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
            })
        }))?;
        Some(CANDIDATES[idx])
    }

    pub fn play(path: &str) {
        if !Path::new(path).exists() {
            return;
        }
        let Some((program, args)) = find_player() else { return };
        let _ = Command::new(program)
            .args(args)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
mod imp {
    pub fn play(_path: &str) {}
}
