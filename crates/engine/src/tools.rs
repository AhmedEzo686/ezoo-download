//! The helper programs that come with Ezoo Download: yt-dlp (finds the video
//! links), ffmpeg (joins video + audio) and aria2 (torrents), all from their
//! GitHub releases. They live in the "tools" folder
//! of the install folder. Once a day, silently, Ezoo Download checks for new
//! versions and updates them, and puts them back if one went missing.
//! On macOS the helpers are installed and updated separately with Homebrew;
//! neither a signed app bundle nor package-managed tools are modified.

use crate::mux::hide_window;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
/// the SHA-256 of every file of that release, published by yt-dlp
const YTDLP_SUMS_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";
/// the SHA-256 of every build, published with the ffmpeg builds
const FFMPEG_SUMS_NAME: &str = "checksums.sha256";
/// none of the helpers is anywhere near this big
const MAX_TOOL_BYTES: u64 = 400 << 20;
/// ffmpeg builds published on GitHub (rolling release "latest")
const FFMPEG_RELEASE_API: &str = "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/tags/latest";
/// aria2 for torrents: one fixed, known build (it changes rarely), checked
/// against this SHA-256 before use
pub const ARIA2_URL: &str = "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip";
pub const ARIA2_ZIP_SHA256: &str = "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288";
pub const ARIA2_EXE_SHA256: &str = "be2099c214f63a3cb4954b09a0becd6e2e34660b886d4c898d260febfe9d70c2";

#[derive(serde::Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(serde::Deserialize)]
struct GhRelease {
    assets: Vec<GhAsset>,
}

/// The newest stable 64-bit Windows build: "ffmpeg-n7.1-latest-win64-gpl-7.1.zip"
/// (the highest n<version>), else the master build.
fn pick_ffmpeg(assets: &[GhAsset]) -> Option<&GhAsset> {
    let re = regex::Regex::new(r"^ffmpeg-n([0-9.]+)-latest-win64-gpl-[0-9.]+\.zip$").ok()?;
    let key = |v: &str| v.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    assets
        .iter()
        .filter_map(|a| re.captures(&a.name).map(|c| (key(&c[1]), a)))
        .max_by(|x, y| x.0.cmp(&y.0))
        .map(|(_, a)| a)
        .or_else(|| assets.iter().find(|a| a.name == "ffmpeg-master-latest-win64-gpl.zip"))
}

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
    if let Ok(executable) = std::env::current_exe() {
        v.extend(app_tool_dirs(&executable));
    }
    if let Some(p) = directories::ProjectDirs::from("com", "Ezoo", "EzooDownload") {
        v.push(p.data_dir().join("tools"));
    }
    v
}

fn app_tool_dirs(executable: &Path) -> Vec<PathBuf> {
    let Some(dir) = executable.parent() else { return Vec::new() };
    let mut dirs = Vec::new();
    #[cfg(target_os = "macos")]
    if dir.file_name().is_some_and(|n| n == "MacOS") {
        if let Some(contents) = dir.parent().filter(|p| p.file_name().is_some_and(|n| n == "Contents")) {
            dirs.push(contents.join("Resources").join("tools"));
        }
    }
    dirs.push(dir.join("tools"));
    dirs.push(dir.to_path_buf());
    dirs
}

/// A regular executable file; directories and Unix files without execute
/// permission must not hide a working copy installed elsewhere.
pub(crate) fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else { return false };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// A tool shipped with the app (not one somewhere on PATH).
pub fn bundled(name: &str) -> Option<PathBuf> {
    tool_dirs().into_iter().map(|d| d.join(exe(name))).find(|p| is_executable(p))
}

/// Installation advice appropriate for the platform running the app.
pub fn missing_tool_message(name: &str) -> String {
    if cfg!(target_os = "macos") {
        let formula = if name == "aria2c" { "aria2" } else { name };
        format!("{name} is missing. Install it with `brew install {formula}`, then try again.")
    } else if cfg!(windows) {
        format!("{name}.exe is missing. Ezoo Download gets it by itself in a minute (internet needed), then try again")
    } else {
        format!("{name} is missing. Install it with your package manager, then try again.")
    }
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
        .send()
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
        .send()
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
        cmd.args(["--ignore-config", "-U"]);
        hide_window(&mut cmd);
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    if ok {
        let _ = std::fs::write(&stamp, b"ok");
    }
}

/// ffmpeg: compare with the newest release build and replace it when
/// there's a newer one (or when it's missing).
async fn ffmpeg(client: &reqwest::Client, data_dir: &Path) -> anyhow::Result<()> {
    if !cfg!(windows) {
        return Ok(());
    }
    let present = bundled("ffmpeg");
    let stamp = data_dir.join("ffmpeg-checked");
    if present.is_some() && !due(&stamp, Duration::from_secs(20 * 3600)) {
        return Ok(());
    }
    let text = client
        .get(FFMPEG_RELEASE_API)
        .header(reqwest::header::USER_AGENT, crate::config::DEFAULT_USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let release: GhRelease = serde_json::from_str(&text)?;
    let Some(asset) = pick_ffmpeg(&release.assets) else {
        anyhow::bail!("no ffmpeg build in the release");
    };
    // name + date: changes whenever a new build is published
    let latest = format!("{} {}", asset.name, asset.updated_at);
    let zip_url = asset.browser_download_url.clone();
    if !zip_url.starts_with("https://github.com/") {
        anyhow::bail!("unexpected ffmpeg download address");
    }
    let sums_url = release
        .assets
        .iter()
        .find(|a| a.name == FFMPEG_SUMS_NAME)
        .map(|a| a.browser_download_url.clone())
        .filter(|u| u.starts_with("https://github.com/"))
        .ok_or_else(|| anyhow::anyhow!("the ffmpeg release has no checksum list"))?;
    let Some(dir) = writable_dir("ffmpeg") else { return Ok(()) };
    let version_file = dir.join("ffmpeg-version.txt");
    let current = std::fs::read_to_string(&version_file).unwrap_or_default();
    if present.is_some() && current.trim() == latest {
        let _ = std::fs::write(&stamp, b"ok");
        return Ok(());
    }
    // download the zip, unpack it with Windows' own tar, take ffmpeg.exe
    let work = std::env::temp_dir().join(format!("ezoo-ffmpeg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let zip = work.join("ffmpeg.zip");
    let sum = sum_for(&get_text(client, &sums_url).await?, &asset.name)
        .ok_or_else(|| anyhow::anyhow!("no checksum for {}", asset.name))?;
    download(client, &zip_url, &zip, &sum).await?;
    let (zip2, work2) = (zip.clone(), work.clone());
    let unpacked = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(crate::system::windows_tool("tar.exe"));
        cmd.arg("-xf").arg(&zip2).arg("-C").arg(&work2);
        hide_window(&mut cmd);
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    let found = unpacked.then(|| find_file(&work, &exe("ffmpeg"))).flatten();
    let result = match found {
        Some(f) => {
            let staged = dir.join("ffmpeg.new");
            std::fs::copy(&f, &staged)?;
            replace(&staged, &dir.join(exe("ffmpeg")))?;
            std::fs::write(&version_file, &latest)?;
            let _ = std::fs::write(&stamp, b"ok");
            Ok(())
        }
        None => Err(anyhow::anyhow!("ffmpeg.exe was not found in the download")),
    };
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// aria2c missing (an older install): get the known build.
async fn aria2(client: &reqwest::Client) -> anyhow::Result<()> {
    if !cfg!(windows) || bundled("aria2c").is_some() {
        return Ok(());
    }
    let Some(dir) = writable_dir("aria2c") else { return Ok(()) };
    let work = std::env::temp_dir().join(format!("ezoo-aria2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let zip = work.join("aria2.zip");
    download(client, ARIA2_URL, &zip, ARIA2_ZIP_SHA256).await?;
    let (zip2, work2) = (zip.clone(), work.clone());
    let unpacked = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(crate::system::windows_tool("tar.exe"));
        cmd.arg("-xf").arg(&zip2).arg("-C").arg(&work2);
        hide_window(&mut cmd);
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    let found = unpacked.then(|| find_file(&work, &exe("aria2c"))).flatten();
    let result = match found {
        Some(f) if sha256_of(&f).as_deref() == Some(ARIA2_EXE_SHA256) => {
            let staged = dir.join("aria2c.new");
            std::fs::copy(&f, &staged)?;
            replace(&staged, &dir.join(exe("aria2c")))
        }
        Some(_) => Err(anyhow::anyhow!("aria2c.exe does not match its known SHA-256")),
        None => Err(anyhow::anyhow!("aria2c.exe was not found in the download")),
    };
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn sha256_of(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let data = std::fs::read(path).ok()?;
    Some(Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect())
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(f) = find_file(&p, name) {
                return Some(f);
            }
        } else if p.file_name().map(|n| n.eq_ignore_ascii_case(name)).unwrap_or(false) {
            return Some(p);
        }
    }
    None
}

/// Check both tools now (only does work when a day has passed or one is
/// missing). Never shows anything; errors just mean "try again later".
pub async fn update_all(client: reqwest::Client, data_dir: PathBuf) {
    if cfg!(target_os = "macos") {
        return;
    }
    yt_dlp(&client, &data_dir).await;
    let _ = ffmpeg(&client, &data_dir).await;
    let _ = aria2(&client).await;
    // leftovers of earlier updates
    for dir in tool_dirs() {
        for name in ["yt-dlp", "ffmpeg", "aria2c"] {
            let _ = std::fs::remove_file(dir.join(exe(name)).with_extension("old"));
        }
    }
}

/// Run `update_all` now and then every few hours while the app is open.
pub fn spawn_updater(rt: &tokio::runtime::Handle, client: reqwest::Client, data_dir: PathBuf) {
    if cfg!(target_os = "macos") {
        return;
    }
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
    #[cfg(target_os = "macos")]
    #[test]
    fn mac_bundle_tools_and_installation_advice() {
        let dirs = app_tool_dirs(Path::new("/Applications/Ezoo Download.app/Contents/MacOS/EzooDownload"));
        assert_eq!(dirs[0], Path::new("/Applications/Ezoo Download.app/Contents/Resources/tools"));
        let plain = app_tool_dirs(Path::new("/tmp/target/release/EzooDownload"));
        assert_eq!(plain[0], Path::new("/tmp/target/release/tools"));
        assert!(missing_tool_message("aria2c").contains("brew install aria2`"));
        assert!(!missing_tool_message("ffmpeg").contains(".exe"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_executable_helpers() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ezoo-executable-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("ffmpeg");
        std::fs::write(&helper, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_executable(&helper));
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&helper));
        assert!(!is_executable(&dir));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn picks_stable_ffmpeg() {
        let a = |n: &str| GhAsset { name: n.into(), browser_download_url: String::new(), updated_at: String::new() };
        let list = vec![
            a("ffmpeg-master-latest-win64-gpl.zip"),
            a("ffmpeg-n7.1-latest-win64-gpl-7.1.zip"),
            a("ffmpeg-n7.0-latest-win64-gpl-7.0.zip"),
            a("ffmpeg-n7.1-latest-win64-gpl-shared-7.1.zip"),
            a("ffmpeg-n7.1-latest-linux64-gpl-7.1.tar.xz"),
        ];
        assert_eq!(pick_ffmpeg(&list).unwrap().name, "ffmpeg-n7.1-latest-win64-gpl-7.1.zip");
        assert_eq!(pick_ffmpeg(&list[..1]).unwrap().name, "ffmpeg-master-latest-win64-gpl.zip");
    }

    #[test]
    fn replace_and_find() {
        let dir = std::env::temp_dir().join(format!("ezoo-tools-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/ffmpeg.exe"), b"new").unwrap();
        assert!(find_file(&dir, "FFMPEG.EXE").is_some());
        let target = dir.join("tool.exe");
        std::fs::write(&target, b"old").unwrap();
        let new = dir.join("tool.new");
        std::fs::write(&new, b"fresh").unwrap();
        replace(&new, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"fresh");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
