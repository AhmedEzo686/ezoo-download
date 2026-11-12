//! The helper programs that come with Ezoo Download: yt-dlp (finds the video
//! links) from its GitHub releases, and our own small ffmpeg build (joins video + audio, MP3; built by tools/ffmpeg/build.sh). They live in the "tools" folder
//! of the install folder. Once a day, silently, Ezoo Download checks for new
//! versions and updates them, and puts them back if one went missing.

use crate::http::SendOrdered;
use crate::mux::hide_window;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
/// the SHA-256 of every file of that release, published by yt-dlp
const YTDLP_SUMS_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";
/// none of the helpers is anywhere near this big
const MAX_TOOL_BYTES: u64 = 400 << 20;

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Folders searched for the tools, best first.
pub fn tool_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        v.push(dir.join("tools"));
        v.push(dir);
    }
    if let Some(p) = directories::ProjectDirs::from("com", "Ezoo", "EzooDownload") {
        v.push(p.data_dir().join("tools"));
    }
    v
}

/// A folder of ours for the helpers' temporary files.
pub fn work_dir() -> Option<PathBuf> {
    let dir = directories::ProjectDirs::from("com", "Ezoo", "EzooDownload")?.data_dir().join("tmp");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// A tool shipped with the app (not one somewhere on PATH).
pub fn bundled(name: &str) -> Option<PathBuf> {
    tool_dirs().into_iter().map(|d| d.join(exe(name))).find(|p| p.exists())
}

/// Where new copies go: the install folder's "tools" when we may write
/// there, else our data folder.
fn writable_dir(name: &str) -> Option<PathBuf> {
    // keep updating the copy that is already used
    if let Some(p) = bundled(name) {
        if let Some(dir) = p.parent() {
            if can_write(dir) {
                return Some(dir.to_path_buf());
            }
        }
    }
    tool_dirs().into_iter().find(|d| std::fs::create_dir_all(d).is_ok() && can_write(d))
}

fn can_write(dir: &Path) -> bool {
    let probe = dir.join(".ezoo-write-test");
    let ok = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

fn due(stamp: &Path, every: Duration) -> bool {
    std::fs::metadata(stamp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|age| age > every)
        .unwrap_or(true)
}

/// The SHA-256 of `name` in a checksum list ("<sha256>  <name>" per line).
pub fn sum_for(list: &str, name: &str) -> Option<String> {
    list.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (sum, file) = (it.next()?, it.next()?);
        (file.trim_start_matches('*') == name && sum.len() == 64 && sum.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| sum.to_ascii_lowercase())
    })
}

async fn get_text(client: &reqwest::Client, url: &str) -> anyhow::Result<String> {
    let text = client
        .get(url)
        .header(reqwest::header::USER_AGENT, crate::config::DEFAULT_USER_AGENT)
        .send_ordered()
        .await?
        .error_for_status()?
        .text()
        .await?;
    if text.len() > 1 << 20 {
        anyhow::bail!("unexpected answer from {url}");
    }
    Ok(text)
}

/// Download `url` to `dest`. It is used only when its SHA-256 is `sha256`
/// (from the checksum list its makers publish): a changed or broken file is
/// thrown away and never run.
async fn download(client: &reqwest::Client, url: &str, dest: &Path, sha256: &str) -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    let mut resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, crate::config::DEFAULT_USER_AGENT)
        .send_ordered()
        .await?
        .error_for_status()?;
    use std::io::Write;
    let tmp = dest.with_extension("download");
    let mut file = std::fs::File::create(&tmp)?;
    let mut h = Sha256::new();
    let mut total: u64 = 0;
    while let Some(chunk) = resp.chunk().await? {
        total += chunk.len() as u64;
        if total > MAX_TOOL_BYTES {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            anyhow::bail!("the download of {url} is far bigger than expected");
        }
        h.update(&chunk);
        file.write_all(&chunk)?;
    }
    drop(file);
    let got: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !got.eq_ignore_ascii_case(sha256) {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("the download of {url} does not match its published SHA-256");
    }
    replace(&tmp, dest)
}

/// Put `new` in place of `target`, even if `target` is running right now
/// (Windows lets us rename a running program, not delete it).
fn replace(new: &Path, target: &Path) -> anyhow::Result<()> {
    if target.exists() {
        let old = target.with_extension("old");
        let _ = std::fs::remove_file(&old);
        if std::fs::remove_file(target).is_err() {
            std::fs::rename(target, &old)?;
        }
    }
    std::fs::rename(new, target)?;
    Ok(())
}

/// yt-dlp missing: get it. Present: `yt-dlp -U` (at most once a day).
async fn yt_dlp(client: &reqwest::Client, data_dir: &Path) {
    if bundled("yt-dlp").is_none() {
        if cfg!(windows) {
            if let Some(dir) = writable_dir("yt-dlp") {
                if let Ok(sums) = get_text(client, YTDLP_SUMS_URL).await {
                    if let Some(sum) = sum_for(&sums, "yt-dlp.exe") {
                        let _ = download(client, YTDLP_URL, &dir.join(exe("yt-dlp")), &sum).await;
                    }
                }
            }
        }
        return;
    }
    let stamp = data_dir.join("yt-dlp-updated");
    if !due(&stamp, Duration::from_secs(20 * 3600)) {
        return;
    }
    let Some(tool) = bundled("yt-dlp") else { return };
    let ok = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(&tool);
        if let Some(dir) = work_dir() {
            cmd.env("TEMP", &dir).env("TMP", &dir).current_dir(&dir);
        }
        cmd.args(["--ignore-config", "-U"]);
        crate::http::with_proxy(&mut cmd);
        hide_window(&mut cmd);
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    if ok {
        let _ = std::fs::write(&stamp, b"ok");
    }
}

/// ffmpeg is our own small build that comes with the setup (and is
/// updated with the program). Copies of the big public build that older
/// versions downloaded by themselves are removed, so ours is the one used.
fn ffmpeg_cleanup() {
    let Some(ours) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("tools").join(exe("ffmpeg")))) else {
        return;
    };
    if !ours.exists() {
        return;
    }
    if let Some(p) = directories::ProjectDirs::from("com", "Ezoo", "EzooDownload") {
        let dir = p.data_dir().join("tools");
        for f in [exe("ffmpeg"), "ffmpeg-version.txt".into(), "ffmpeg-LICENSE.txt".into()] {
            let _ = std::fs::remove_file(dir.join(f));
        }
    }
}

/// The torrent helper of older versions: torrents are shared by the program
/// itself now.
fn aria2_cleanup() {
    for dir in tool_dirs() {
        for f in [exe("aria2c"), "aria2-COPYING.txt".into(), "aria2c.new".into()] {
            let _ = std::fs::remove_file(dir.join(f));
        }
    }
}

/// The SHA-256 of the ffmpeg.exe that GitHub built together with this very
/// version (written into the program by the build; none in a home build).
const FFMPEG_SHA256: Option<&str> = option_env!("EZOO_FFMPEG_SHA256");

/// ffmpeg went missing (an antivirus, a broken setup...): the copy of this
/// release is fetched again, and used only when its SHA-256 is the one this
/// program was built with. true = ffmpeg is there now.
pub async fn restore_ffmpeg(client: &reqwest::Client) -> bool {
    if bundled("ffmpeg").is_some() {
        return true;
    }
    let Some(sum) = FFMPEG_SHA256.filter(|s| s.len() == 64) else {
        return false;
    };
    if !cfg!(windows) {
        return false;
    }
    let Some(dir) = writable_dir("ffmpeg") else { return false };
    let repo = crate::selfupdate::REPO;
    let urls = [
        format!("https://github.com/{repo}/releases/download/v{}/ffmpeg.exe", env!("CARGO_PKG_VERSION")),
        format!("https://github.com/{repo}/releases/latest/download/ffmpeg.exe"),
    ];
    for url in urls {
        if download(client, &url, &dir.join(exe("ffmpeg")), sum).await.is_ok() {
            return true;
        }
    }
    false
}

/// Check both tools now (only does work when a day has passed or one is
/// missing). Never shows anything; errors just mean "try again later".
pub async fn update_all(client: reqwest::Client, data_dir: PathBuf) {
    yt_dlp(&client, &data_dir).await;
    restore_ffmpeg(&client).await;
    ffmpeg_cleanup();
    aria2_cleanup();
    // leftovers of earlier updates
    for dir in tool_dirs() {
        for name in ["yt-dlp", "ffmpeg"] {
            let _ = std::fs::remove_file(dir.join(exe(name)).with_extension("old"));
        }
    }
}

/// Run `update_all` now and then every few hours while the app is open.
pub fn spawn_updater(rt: &tokio::runtime::Handle, client: reqwest::Client, data_dir: PathBuf) {
    rt.spawn(async move {
        // let the app start first
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            update_all(client.clone(), data_dir.clone()).await;
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replace_a_tool() {
        let dir = std::env::temp_dir().join(format!("ezoo-tools-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("tool.exe");
        std::fs::write(&target, b"old").unwrap();
        let new = dir.join("tool.new");
        std::fs::write(&new, b"fresh").unwrap();
        replace(&new, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"fresh");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
