//! Joining a video-only and an audio-only file into one playable file.
//! Uses ffmpeg in "copy" mode: no re-encoding, so it takes seconds and keeps
//! the original quality.

use anyhow::{bail, Context};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A helper program (ffmpeg, yt-dlp): the copy in the app's "tools"
/// folder wins, then one next to EzooDownload.exe, then PATH.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    if let Some(p) = crate::tools::bundled(name) {
        return Some(p);
    }
    if let Ok(me) = std::env::current_exe() {
        let local = me.with_file_name(&exe_name);
        if local.exists() {
            return Some(local);
        }
        let in_folder = me.with_file_name(name).join("bin").join(&exe_name);
        if in_folder.exists() {
            return Some(in_folder);
        }
    }
    let mut probe = Command::new(&exe_name);
    probe.arg("--version");
    hide_window(&mut probe);
    match probe.output() {
        Ok(o) if o.status.success() => Some(PathBuf::from(exe_name)),
        // ffmpeg prints its version with -version and fails on --version
        Ok(_) if name == "ffmpeg" => {
            let mut p = Command::new(&exe_name);
            p.arg("-version");
            hide_window(&mut p);
            p.output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from(&exe_name))
        }
        _ => None,
    }
}

pub fn find_ffmpeg() -> Option<PathBuf> {
    find_tool("ffmpeg")
}

/// ffmpeg muxer name for the final file extension. The output path ends in
/// `.sgpart` while downloading, so ffmpeg can't guess it from the name.
pub fn format_for(file_name: &str) -> &'static str {
    match crate::filename::extension(file_name).as_str() {
        "webm" => "webm",
        "mkv" => "matroska",
        "m4a" | "mp4" | "m4v" | "mov" => "mp4",
        _ => "matroska",
    }
}

pub fn merge(video: &Path, audio: &Path, output: &Path, final_name: &str) -> anyhow::Result<()> {
    let Some(ffmpeg) = find_ffmpeg() else {
        bail!("{MISSING}");
    };
    let format = format_for(final_name);
    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(video)
        .arg("-i")
        .arg(audio)
        .args(["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"]);
    if format == "mp4" {
        // play while still loading, like normal web videos
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.args(["-f", format]).arg(output);
    hide_window(&mut cmd);
    let out = cmd.output().context("could not start ffmpeg")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
        bail!("ffmpeg could not merge video and audio: {last}");
    }
    Ok(())
}

/// What to say when our ffmpeg.exe is not there (it comes with the setup).
const MISSING: &str = "ffmpeg.exe is missing from the \"tools\" folder of Ezoo Download: install Ezoo Download again to get it back";

/// A joined stream (HLS, a .ts file) into an ordinary MP4 file: plays in
/// every player, with seeking. Nothing is re-encoded.
pub fn ts_to_mp4(input: &Path, output: &Path) -> anyhow::Result<()> {
    let Some(ffmpeg) = find_ffmpeg() else {
        bail!("{MISSING}");
    };
    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "mpegts", "-i"])
        .arg(input)
        .args(["-map", "0:v?", "-map", "0:a?", "-c", "copy", "-movflags", "+faststart", "-f", "mp4"])
        .arg(output);
    hide_window(&mut cmd);
    let out = cmd.output().context("could not start ffmpeg")?;
    let made = std::fs::metadata(output).map(|m| m.len() > 0).unwrap_or(false);
    if !out.status.success() || !made {
        let _ = std::fs::remove_file(output);
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
        bail!("ffmpeg could not make the MP4 file: {last}");
    }
    Ok(())
}

/// "Play while downloading" for picture + sound in two files: one playable
/// file of what is there so far (the first `video_ready` / `audio_ready`
/// bytes, nothing is re-encoded, takes a moment). `output` gets a
/// Matroska (.mkv) file: it takes every kind of picture and sound.
pub fn preview_snapshot(video: &Path, video_ready: u64, audio: Option<(&Path, u64)>, output: &Path) -> anyhow::Result<()> {
    let Some(ffmpeg) = find_ffmpeg() else {
        bail!("{MISSING}");
    };
    // only the part with no gap ("subfile": ffmpeg reads just those bytes)
    let part = |p: &Path, n: u64| {
        let mut s = std::ffi::OsString::from(format!("subfile,,start,0,end,{n},,:"));
        s.push(p.as_os_str());
        s
    };
    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-protocol_whitelist", "file,subfile", "-i"])
        .arg(part(video, video_ready));
    if let Some((a, n)) = audio {
        cmd.arg("-i").arg(part(a, n)).args(["-map", "0:v:0", "-map", "1:a:0"]);
    } else {
        cmd.args(["-map", "0:v:0"]);
    }
    cmd.args(["-c", "copy", "-f", "matroska"]).arg(output);
    hide_window(&mut cmd);
    let out = cmd.output().context("could not start ffmpeg")?;
    // the cut at the end makes ffmpeg complain; what it wrote is fine
    let made = std::fs::metadata(output).map(|m| m.len() > 1024).unwrap_or(false);
    if !made {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
        bail!("ffmpeg could not prepare the preview: {last}");
    }
    Ok(())
}

/// Turn a downloaded sound (or video) file into an MP3 file.
pub fn to_mp3(input: &Path, output: &Path, kbps: u64, title: Option<&str>) -> anyhow::Result<()> {
    let Some(ffmpeg) = find_ffmpeg() else {
        bail!("{MISSING}");
    };
    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(["-vn", "-map", "0:a:0", "-map_metadata", "-1", "-c:a", "libmp3lame", "-b:a"])
        .arg(format!("{kbps}k"))
        // the name shows on phones, car radios and players
        .args(["-id3v2_version", "3"]);
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        cmd.arg("-metadata").arg(format!("title={}", t.chars().take(200).collect::<String>()));
    }
    cmd.args(["-f", "mp3"]).arg(output);
    hide_window(&mut cmd);
    let out = cmd.output().context("could not start ffmpeg")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
        bail!("ffmpeg could not make the MP3 file: {last}");
    }
    Ok(())
}

pub(crate) fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_of_half_downloaded_video_and_sound() {
        let Some(ffmpeg) = find_ffmpeg() else { return };
        let dir = std::env::temp_dir().join(format!("swiftget-preview-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let (v, a) = (dir.join(".x.video.sgpart"), dir.join(".x.audio.sgpart"));
        let gen = |args: &[&str], out: &Path| {
            let ok = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).arg(out).status().unwrap();
            assert!(ok.success());
        };
        // like YouTube: fragmented MP4, picture and sound apart
        gen(&["-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=20", "-c:v", "mpeg4", "-an", "-movflags", "+frag_keyframe+empty_moov", "-f", "mp4"], &v);
        gen(&["-f", "lavfi", "-i", "sine=frequency=440:duration=20", "-c:a", "aac", "-vn", "-movflags", "+frag_keyframe+empty_moov", "-f", "mp4"], &a);
        // the files are full size already (sparse), only half is real
        let (vl, al) = (std::fs::metadata(&v).unwrap().len(), std::fs::metadata(&a).unwrap().len());
        let out = dir.join("x (preview).mkv");
        preview_snapshot(&v, vl / 2, Some((&a, al / 2)), &out).unwrap();
        let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
        let info = String::from_utf8_lossy(&probe.stderr);
        assert!(info.contains("Video: mpeg4") && info.contains("Audio: aac"), "{info}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stream_to_mp4() {
        let Some(ffmpeg) = find_ffmpeg() else { return };
        let dir = std::env::temp_dir().join(format!("swiftget-ts-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let ts = dir.join("x.ts.sgpart");
        let ok = Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=2", "-f", "lavfi", "-i", "sine=duration=2", "-c:v", "mpeg4", "-c:a", "aac", "-f", "mpegts"])
            .arg(&ts)
            .status()
            .unwrap();
        assert!(ok.success());
        let out = dir.join("x.mp4.tmp");
        ts_to_mp4(&ts, &out).unwrap();
        let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
        let info = String::from_utf8_lossy(&probe.stderr);
        assert!(info.contains("mp4") && info.contains("Audio: aac"), "{info}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn makes_mp3() {
        let Some(ffmpeg) = find_ffmpeg() else { return };
        let dir = std::env::temp_dir().join("swiftget-mp3-test");
        let _ = std::fs::create_dir_all(&dir);
        let a = dir.join("a.m4a.sgpart");
        let ok = Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:a", "aac", "-f", "mp4"])
            .arg(&a)
            .status()
            .unwrap();
        assert!(ok.success());
        let out = dir.join("song.mp3.tmp");
        to_mp3(&a, &out, 192, Some("Song")).unwrap();
        let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
        let info = String::from_utf8_lossy(&probe.stderr);
        assert!(info.contains("Audio: mp3") && info.contains("Song"), "{info}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Real merge, only when ffmpeg is installed (skipped otherwise).
    #[test]
    fn merges_video_and_audio() {
        let Some(ffmpeg) = find_ffmpeg() else { return };
        let dir = std::env::temp_dir().join("swiftget-mux-test");
        let _ = std::fs::create_dir_all(&dir);
        let (v, a) = (dir.join("v.mp4"), dir.join("a.m4a"));
        let gen = |args: &[&str]| {
            let ok = Command::new(&ffmpeg).args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).status().unwrap();
            assert!(ok.success());
        };
        gen(&["-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=2", "-c:v", "mpeg4", "-an", v.to_str().unwrap()]);
        gen(&["-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:a", "aac", "-vn", a.to_str().unwrap()]);
        // the output still has the .sgpart name while downloading
        let out = dir.join("Clip [1080p].mp4.sgpart");
        merge(&v, &a, &out, "Clip [1080p].mp4").unwrap();
        let probe = Command::new(&ffmpeg).arg("-i").arg(&out).output().unwrap();
        let info = String::from_utf8_lossy(&probe.stderr);
        assert!(info.contains("Video: mpeg4") && info.contains("Audio: aac"), "{info}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn formats() {
        assert_eq!(format_for("a [1080p].mp4"), "mp4");
        assert_eq!(format_for("a.webm"), "webm");
        assert_eq!(format_for("a.mkv"), "matroska");
    }
}
