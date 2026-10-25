//! "Play while downloading": open the part of a video / song that is
//! already here in the user's player, while the rest keeps coming.
//!
//! The file still has its temporary name (".sgpart"), so Windows would not
//! know which program plays it. The program set for the real extension
//! (.mp4, .mkv, .mp3...) is asked for and started with the file. Store apps
//! (Movies & TV, Media Player) can't be started that way: for them a second
//! name of the same file with the real extension is made (a hard link, no
//! copy, it grows with the download) and opened; it goes away by itself.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Second names made for previews, removed when no player holds them.
static LINKS: Mutex<Vec<(PathBuf, std::time::Instant)>> = Mutex::new(Vec::new());

/// Play `part` (the file being downloaded) as a `final_name` file.
pub(crate) fn play(part: &Path, final_name: &str) -> std::io::Result<()> {
    let ext = swiftget_engine::filename::extension(final_name);
    #[cfg(windows)]
    if let Some(player) = player_for(&ext) {
        std::process::Command::new(player).arg(part).spawn()?;
        return Ok(());
    }
    let link = free_name(link_name(part, final_name, &ext))?;
    std::fs::hard_link(part, &link)?;
    LINKS.lock().unwrap_or_else(|p| p.into_inner()).push((link.clone(), std::time::Instant::now()));
    open::that_detached(&link)
}

/// Never over a file that is already there (it could be the user's own).
fn free_name(first: PathBuf) -> std::io::Result<PathBuf> {
    let mut link = first;
    let mut n = 2;
    while link.exists() {
        let (stem, e) = (link.file_stem().unwrap_or_default().to_string_lossy().into_owned(), link.extension().unwrap_or_default().to_string_lossy().into_owned());
        let base = stem.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ').to_string();
        link = link.with_file_name(format!("{base} {n}.{e}"));
        n += 1;
        if n > 50 {
            return Err(std::io::Error::other("too many preview files"));
        }
    }
    Ok(link)
}

/// Picture and sound in two files (YouTube): one file of what is there so
/// far is made (a few seconds, nothing re-encoded) and opened. It is a
/// picture of this moment: "Play while downloading" again shows more.
/// Runs on its own thread; `done` gets the error, if any.
pub(crate) fn play_pair(
    video: PathBuf,
    video_ready: u64,
    audio: Option<(PathBuf, u64)>,
    final_name: &str,
    done: impl FnOnce(Option<String>) + Send + 'static,
) {
    let first = link_name(&video, final_name, "mkv");
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<()> {
            let out = free_name(first)?;
            swiftget_engine::mux::preview_snapshot(&video, video_ready, audio.as_ref().map(|(p, n)| (p.as_path(), *n)), &out)?;
            LINKS.lock().unwrap_or_else(|p| p.into_inner()).push((out.clone(), std::time::Instant::now()));
            open::that_detached(&out)?;
            Ok(())
        })();
        done(result.err().map(|e| format!("{e:#}")));
    });
}

/// Remove the second names no player uses any more. A player that is
/// still starting gets half a minute to open its file.
pub(crate) fn clean_up() {
    clean(std::time::Duration::from_secs(30));
}

/// At exit: every name a player does not hold open right now.
pub(crate) fn clean_all() {
    clean(std::time::Duration::ZERO);
}

fn clean(grace: std::time::Duration) {
    let mut links = LINKS.lock().unwrap_or_else(|p| p.into_inner());
    links.retain(|(p, made)| p.exists() && (made.elapsed() < grace || std::fs::remove_file(p).is_err()));
}

fn link_name(part: &Path, final_name: &str, ext: &str) -> PathBuf {
    let stem = Path::new(final_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "video".into());
    let ext = if ext.is_empty() { "mp4" } else { ext };
    part.with_file_name(format!("{stem} (preview).{ext}"))
}

/// The program Windows uses to open files with this extension, if it is an
/// ordinary program (not a Store app, not a stand-in like rundll32).
#[cfg(windows)]
fn player_for(ext: &str) -> Option<PathBuf> {
    use windows_sys::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_INIT_IGNOREUNKNOWN, ASSOCF_NOTRUNCATE, ASSOCSTR_EXECUTABLE};
    if ext.is_empty() {
        return None;
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (dot_ext, verb) = (wide(&format!(".{ext}")), wide("open"));
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: the strings end in 0 and live through the call; `buf` has
    // room for `len` characters, which Windows does not go past.
    let hr = unsafe {
        AssocQueryStringW(
            ASSOCF_INIT_IGNOREUNKNOWN | ASSOCF_NOTRUNCATE,
            ASSOCSTR_EXECUTABLE,
            dot_ext.as_ptr(),
            verb.as_ptr(),
            buf.as_mut_ptr(),
            &mut len,
        )
    };
    if hr != 0 {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let exe = PathBuf::from(String::from_utf16_lossy(&buf[..end]));
    let lower = exe.to_string_lossy().to_ascii_lowercase();
    let stand_in = ["rundll32", "explorer.exe", "openwith", "\\windowsapps\\", "launchwinapp"].iter().any(|k| lower.contains(k));
    (!stand_in && lower.ends_with(".exe") && exe.is_file()).then_some(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_name_has_the_real_extension() {
        let part = Path::new("/tmp/x/.Clip.mp4.sgpart");
        assert_eq!(link_name(part, "Clip [1080p].mp4", "mp4"), Path::new("/tmp/x/Clip [1080p] (preview).mp4"));
        assert_eq!(link_name(part, "song", ""), Path::new("/tmp/x/song (preview).mp4"));
    }

    #[test]
    fn plays_through_a_second_name() {
        let dir = std::env::temp_dir().join(format!("ezoo-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("a.mp4.sgpart");
        std::fs::write(&part, b"data").unwrap();
        let link = link_name(&part, "a.mp4", "mp4");
        std::fs::hard_link(&part, &link).unwrap();
        // the same file: what the download writes is seen through the link
        std::fs::write(&part, b"more data").unwrap();
        assert_eq!(std::fs::read(&link).unwrap(), b"more data");
        LINKS.lock().unwrap().push((link.clone(), std::time::Instant::now()));
        clean_up();
        assert!(link.exists(), "a player that is still starting keeps it");
        clean_all();
        assert!(!link.exists() && part.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
