//! "Download a part of a video": ffmpeg cuts the part out, either from a
//! file here (a stream whose parts were fetched) or from our own address
//! of the site's video (clipserve), so only that part is downloaded.
//!
//! Two kinds of cut:
//! * quick (the usual one): nothing is made again, it takes seconds and keeps
//!   the quality as it is, but a video can only start on a whole picture,
//!   so the part may start a second or two before the time asked;
//! * exact: the picture is made again (H.264) and the part starts on the
//!   very second asked. Slower, and it works the processor.
//!
//! Sound alone is always cut on the very second.

use anyhow::{anyhow, bail, Context};
use std::path::Path;
use std::process::Stdio;
use tokio_util::sync::CancellationToken;

/// What the part becomes.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// picture and sound (MP4)
    Video,
    /// MP3 at this many kb/s, with this title
    Mp3(u64, Option<String>),
    /// sound alone in an MP4 file (.m4a)
    M4a,
    /// sound alone in an Ogg file (Opus)
    Ogg,
}

impl Output {
    /// From the file's name (and "Audio · MP3").
    pub fn for_name(name: &str, mp3: Option<(u64, String)>) -> Output {
        if let Some((kbps, title)) = mp3 {
            return Output::Mp3(kbps, Some(title));
        }
        match crate::filename::extension(name).as_str() {
            "mp3" => Output::Mp3(crate::ytdlp::mp3_kbps(), None),
            "m4a" | "aac" => Output::M4a,
            "ogg" | "opus" => Output::Ogg,
            _ => Output::Video,
        }
    }
    fn is_video(&self) -> bool {
        *self == Output::Video
    }
}

/// How the cut went.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Done {
    /// as asked
    AsAsked,
    /// the exact cut can't be done for this video (its kind of picture
    /// can't be read here): it was cut the quick way
    QuickInstead,
}

/// What ffmpeg says about the first video of an input.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Picture {
    pub codec: String,
    pub width: u32,
    pub height: u32,
}

/// "Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv), 1280x720 [SAR 1:1 DAR 16:9], ..."
pub fn parse_picture(ffmpeg_says: &str) -> Option<Picture> {
    let line = ffmpeg_says.lines().find(|l| l.contains("Stream #") && l.contains(": Video: "))?;
    let after = line.split(": Video: ").nth(1)?;
    let codec = after.split([' ', ',']).next().unwrap_or("").to_string();
    let size = after
        .split([',', ' '])
        .map(str::trim)
        .find_map(|w| {
            let (a, b) = w.split_once('x')?;
            Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))
        })
        .unwrap_or((0, 0));
    Some(Picture { codec, width: size.0, height: size.1 })
}

/// "Duration: 00:03:25.48," -> 205.48
pub fn parse_duration(ffmpeg_says: &str) -> Option<f64> {
    let at = ffmpeg_says.find("Duration: ")? + "Duration: ".len();
    let t = ffmpeg_says[at..].split(',').next()?.trim();
    let mut secs = 0.0;
    for p in t.split(':') {
        secs = secs * 60.0 + p.trim().parse::<f64>().ok()?;
    }
    (secs > 0.0).then_some(secs)
}

/// Kinds of picture our ffmpeg reads (to make them again for the exact cut).
fn readable(codec: &str) -> bool {
    matches!(codec, "h264" | "hevc" | "vp8" | "vp9" | "mpeg4")
}

/// The H.264 maker of this ffmpeg (ours has OpenH264).
fn h264_maker(ffmpeg: &Path) -> &'static str {
    static FOUND: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    FOUND.get_or_init(|| {
        let mut cmd = std::process::Command::new(ffmpeg);
        cmd.args(["-hide_banner", "-encoders"]);
        crate::mux::hide_window(&mut cmd);
        let list = cmd.output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        ["libopenh264", "libx264", "h264_mf"].into_iter().find(|e| list.contains(&format!(" {e} "))).unwrap_or("libopenh264")
    })
}

/// Picture quality of the exact cut (kb/s) for a picture of this size.
fn video_kbps(p: &Picture) -> u64 {
    let pixels = (p.width as u64 * p.height as u64).max(640 * 360);
    (pixels * 8000 / (1920 * 1080)).clamp(1200, 40_000)
}

fn command(ffmpeg: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(ffmpeg);
    crate::mux::hide_window(&mut cmd);
    crate::http::with_proxy(&mut cmd);
    cmd.stdin(Stdio::null());
    cmd
}

/// What ffmpeg sees in `input`: how long it plays and its picture.
pub async fn look(ffmpeg: &Path, input: &str) -> anyhow::Result<(Option<f64>, Option<Picture>)> {
    let (ffmpeg, input) = (ffmpeg.to_path_buf(), input.to_string());
    let out = tokio::task::spawn_blocking(move || {
        command(&ffmpeg).args(["-hide_banner", "-nostdin", "-i", &input]).stdout(Stdio::null()).stderr(Stdio::piped()).output()
    })
    .await?
    .context("could not start ffmpeg")?;
    // "At least one output file must be specified": that is fine here
    let said = String::from_utf8_lossy(&out.stderr);
    if !said.contains("Input #0") {
        let last = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
        bail!("ffmpeg could not read the video: {last}");
    }
    Ok((parse_duration(&said), parse_picture(&said)))
}

/// Run ffmpeg with `args` + `output`; `done(seconds made)` while it works;
/// stopped at once when `cancel` fires.
async fn run_ffmpeg(ffmpeg: &Path, args: Vec<String>, output: &Path, done: &(dyn Fn(f64) + Sync), cancel: &CancellationToken) -> anyhow::Result<()> {
    use std::io::{BufRead, Read};
    let mut child = command(ffmpeg).args(&args).arg(output).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().context("could not start ffmpeg")?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("ffmpeg gave no output"))?;
    let mut stderr = child.stderr.take().ok_or_else(|| anyhow!("ffmpeg gave no output"))?;
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    // how far it is, from its "-progress" lines
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<f64>();
    let reader = std::thread::spawn(move || {
        for l in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(us) = l.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<i64>().ok()) {
                let _ = tx.send(us.max(0) as f64 / 1e6);
            }
        }
    });
    let child = std::sync::Arc::new(std::sync::Mutex::new(child));
    loop {
        tokio::select! {
            got = rx.recv() => match got {
                Some(secs) => done(secs),
                None => break,
            },
            _ = cancel.cancelled() => {
                let _ = child.lock().unwrap().kill();
                let _ = child.lock().unwrap().wait();
                return Err(crate::http_download::Cancelled.into());
            }
        }
    }
    let _ = reader.join();
    let c2 = child.clone();
    let status = tokio::task::spawn_blocking(move || c2.lock().unwrap().wait()).await??;
    let said = errors.join().unwrap_or_default();
    let made = std::fs::metadata(output).map(|m| m.len() > 0).unwrap_or(false);
    if !status.success() || !made {
        let last = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error").to_string();
        bail!("ffmpeg could not cut the part: {last}");
    }
    Ok(())
}

/// The ffmpeg words for one cut (without the program, the inputs' place
/// in `inputs` and the output file).
pub fn cut_args(inputs: &[String], from: f64, len: f64, exact_with: Option<(&str, u64)>, out: &Output) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-nostats", "-progress", "pipe:1"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    for i in inputs {
        a.extend(["-ss".into(), format!("{from:.3}"), "-i".into(), i.clone()]);
    }
    a.extend(["-t".into(), format!("{len:.3}")]);
    let two = inputs.len() > 1;
    if out.is_video() {
        if two {
            a.extend(["-map", "0:v:0", "-map", "1:a:0?"].map(String::from));
        } else {
            a.extend(["-map", "0:v:0?", "-map", "0:a:0?"].map(String::from));
        }
        match exact_with {
            Some((maker, kbps)) => {
                a.extend(["-c:v".into(), maker.into(), "-b:v".into(), format!("{kbps}k"), "-maxrate".into(), format!("{}k", kbps * 3 / 2)]);
                if maker == "libx264" {
                    a.extend(["-preset", "veryfast"].map(String::from));
                }
                a.extend(["-vf", "format=yuv420p", "-c:a", "aac", "-b:a", "192k"].map(String::from));
            }
            None => a.extend(["-c", "copy", "-avoid_negative_ts", "make_zero"].map(String::from)),
        }
        a.extend(["-movflags", "+faststart", "-f", "mp4"].map(String::from));
    } else {
        let sound = if two { "1:a:0" } else { "0:a:0" };
        a.extend(["-vn".into(), "-map".into(), sound.into(), "-map_metadata".into(), "-1".into()]);
        match out {
            Output::Mp3(kbps, title) => {
                a.extend(["-c:a".into(), "libmp3lame".into(), "-b:a".into(), format!("{kbps}k"), "-id3v2_version".into(), "3".into()]);
                if let Some(t) = title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                    a.extend(["-metadata".into(), format!("title={}", t.chars().take(200).collect::<String>())]);
                }
                a.extend(["-f", "mp3"].map(String::from));
            }
            Output::M4a => a.extend(["-c:a", "copy", "-movflags", "+faststart", "-f", "mp4"].map(String::from)),
            _ => a.extend(["-c:a", "copy", "-f", "ogg"].map(String::from)),
        }
    }
    a
}

/// Cut `len` seconds from `from` of `inputs` (a picture, and maybe its
/// sound apart) into `output`. `done(fraction)` while it works.
#[allow(clippy::too_many_arguments)]
pub async fn cut(
    ffmpeg: &Path,
    inputs: &[String],
    from: f64,
    len: f64,
    exact: bool,
    out: &Output,
    output: &Path,
    done: impl Fn(f64) + Sync,
    cancel: &CancellationToken,
) -> anyhow::Result<Done> {
    if inputs.is_empty() || len <= 0.0 {
        bail!("nothing to cut");
    }
    let mut result = Done::AsAsked;
    let mut exact_with = None;
    if exact && out.is_video() {
        let pic = look(ffmpeg, &inputs[0]).await.ok().and_then(|(_, p)| p);
        match pic {
            Some(p) if readable(&p.codec) => exact_with = Some((h264_maker(ffmpeg), video_kbps(&p))),
            _ => result = Done::QuickInstead,
        }
    }
    let progress = move |secs: f64| done((secs / len).clamp(0.0, 1.0));
    match run_ffmpeg(ffmpeg, cut_args(inputs, from, len, exact_with, out), output, &progress, cancel).await {
        Ok(()) => Ok(result),
        Err(e) if e.is::<crate::http_download::Cancelled>() => Err(e),
        // the picture could not be made again: the quick cut
        Err(_) if exact_with.is_some() => {
            run_ffmpeg(ffmpeg, cut_args(inputs, from, len, None, out), output, &progress, cancel).await?;
            Ok(Done::QuickInstead)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_ffmpeg_says() {
        let said = "Input #0, mov,mp4, from 'x':\n  Duration: 00:03:25.48, start: 0.000000, bitrate: 1200 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709), 1280x720 [SAR 1:1 DAR 16:9], 1000 kb/s, 30 fps\n  Stream #0:1: Audio: aac";
        assert_eq!(parse_duration(said), Some(205.48));
        assert_eq!(parse_picture(said), Some(Picture { codec: "h264".into(), width: 1280, height: 720 }));
        assert!(parse_picture("Stream #0:0: Audio: opus").is_none());
        assert_eq!(parse_duration("Duration: N/A, bitrate"), None);
    }

    #[test]
    fn quick_and_exact_words() {
        let ins = vec!["v".to_string(), "a".to_string()];
        let q = cut_args(&ins, 10.0, 5.0, None, &Output::Video).join(" ");
        assert!(q.contains("-ss 10.000 -i v -ss 10.000 -i a -t 5.000"));
        assert!(q.contains("-map 0:v:0 -map 1:a:0? -c copy"));
        let e = cut_args(&ins[..1], 10.0, 5.0, Some(("libopenh264", 4000)), &Output::Video).join(" ");
        assert!(e.contains("-c:v libopenh264 -b:v 4000k") && e.contains("-c:a aac"));
        let m = cut_args(&ins[..1], 1.0, 2.0, None, &Output::Mp3(192, Some("Song".into()))).join(" ");
        assert!(m.contains("-vn -map 0:a:0") && m.contains("libmp3lame -b:a 192k") && m.contains("title=Song"));
        assert_eq!(video_kbps(&Picture { codec: "h264".into(), width: 1920, height: 1080 }), 8000);
    }
}
