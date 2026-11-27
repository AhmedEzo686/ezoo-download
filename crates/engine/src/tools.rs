//! The helper programs that come with Ezoo Download: yt-dlp (finds the video
//! links) from its GitHub releases, and our own small ffmpeg build (joins video + audio, MP3; built by tools/ffmpeg/build.sh). They live in the "tools" folder
//! of the install folder. Once a day, silently, Ezoo Download checks for new
//! versions and updates them, and puts them back if one went missing.

use crate::http::SendOrdered;
use crate::mux::hide_window;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

/// yt-dlp's one-file program for this Windows: 64-bit, or 32-bit in the
/// 32-bit build (Windows 10 32-bit). Kept as yt-dlp.exe either way.
#[cfg(not(target_arch = "x86"))]
const YTDLP_EXE: &str = "yt-dlp.exe";
#[cfg(target_arch = "x86")]
const YTDLP_EXE: &str = "yt-dlp_x86.exe";
const YTDLP_RELEASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/";
/// the SHA-256 of every file of that release, published by yt-dlp
const YTDLP_SUMS_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";
/// none of the helpers is anywhere near this big
const MAX_TOOL_BYTES: u64 = 400 << 20;

/// yt-dlp's release file for this system.
fn ytdlp_file() -> &'static str {
    if cfg!(windows) {
        "yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else if cfg!(target_arch = "aarch64") {
        "yt-dlp_linux_aarch64"
    } else {
        "yt-dlp_linux"
    }
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// A downloaded helper must be runnable (Linux / macOS keep the file mode
/// of what we write, and that is not "x").
#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

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
    #[cfg(feature = "legacy")]
    if name == "yt-dlp" {
        return legacy::ytdlp();
    }
    tool_dirs().into_iter().map(|d| d.join(exe(name))).find(|p| p.exists())
}

/// The command that runs yt-dlp `tool` (what `bundled`/`find_tool` gave):
/// the program itself, or on Windows 7 / 8 / 8.1 our Python with
/// yt-dlp's one-file Python build (yt-dlp's programs need Windows 10).
pub fn ytdlp_base_command(tool: &Path) -> Command {
    #[cfg(feature = "legacy")]
    if let Some(cmd) = legacy::command(tool) {
        return cmd;
    }
    Command::new(tool)
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

/// yt-dlp's folder build: the same yt-dlp, unpacked once. The one-file
/// yt-dlp.exe unpacks its Python into a new temporary folder every time it
/// starts (and the antivirus looks at all of it each time): a second or
/// more before every look at a video. Kept as tools\yt-dlp-dir\<version>\,
/// the one in use named in "current.txt".
/// yt-dlp's folder build and the program in it (named for its kind: the
/// 32-bit one is yt-dlp_x86.exe)
#[cfg(not(target_arch = "x86"))]
const YTDLP_DIR_ZIP: &str = "yt-dlp_win.zip";
#[cfg(target_arch = "x86")]
const YTDLP_DIR_ZIP: &str = "yt-dlp_win_x86.zip";

fn folder_build_root() -> Option<PathBuf> {
    tool_dirs().into_iter().map(|d| d.join("yt-dlp-dir")).find(|d| d.join("current.txt").is_file())
}

/// yt-dlp.exe of the folder build, when it is there.
pub fn ytdlp_folder_build() -> Option<PathBuf> {
    // Windows 10 and newer only
    if !cfg!(windows) || cfg!(feature = "legacy") {
        return None;
    }
    let root = folder_build_root()?;
    let name = std::fs::read_to_string(root.join("current.txt")).ok()?;
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let exe = root.join(name).join(YTDLP_EXE);
    (exe.is_file() && root.join(name).join("_internal").is_dir()).then_some(exe)
}

/// Get the newest folder build when it is not there yet (or yt-dlp made a
/// new one): checked against yt-dlp's own list of SHA-256, unpacked to one
/// side, then made the one in use. Older ones go when nothing runs them.
async fn yt_dlp_folder(client: &reqwest::Client, sums: &str) -> bool {
    let Some(sum) = sum_for(sums, YTDLP_DIR_ZIP) else { return false };
    let version = &sum[..16];
    let root = match folder_build_root() {
        Some(r) => r,
        None => match writable_dir("yt-dlp") {
            Some(d) => d.join("yt-dlp-dir"),
            None => return false,
        },
    };
    let current = std::fs::read_to_string(root.join("current.txt")).unwrap_or_default();
    if current.trim() == version && ytdlp_folder_build().is_some() {
        return true;
    }
    if std::fs::create_dir_all(&root).is_err() {
        return false;
    }
    let zip = root.join("download.zip");
    if download(client, &format!("{YTDLP_RELEASE}{YTDLP_DIR_ZIP}"), &zip, &sum).await.is_err() {
        return false;
    }
    let (root2, zip2, version2) = (root.clone(), zip.clone(), version.to_string());
    let ok = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let bytes = std::fs::read(&zip2)?;
        let part = root2.join(format!("{version2}.part"));
        let _ = std::fs::remove_dir_all(&part);
        crate::unzip::unpack(&bytes, &part, 600 << 20)?;
        let _ = std::fs::remove_file(&zip2);
        if !part.join(YTDLP_EXE).is_file() || !part.join("_internal").is_dir() {
            let _ = std::fs::remove_dir_all(&part);
            anyhow::bail!("not yt-dlp's folder build");
        }
        let fin = root2.join(&version2);
        let _ = std::fs::remove_dir_all(&fin);
        std::fs::rename(&part, &fin)?;
        std::fs::write(root2.join("current.txt"), &version2)?;
        Ok(())
    })
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false);
    let _ = std::fs::remove_file(&zip);
    if ok {
        old_folder_builds();
    }
    ok
}

/// "Update video downloader": a new folder build now, if yt-dlp made one.
/// Blocking. true = the folder build is there (new or already newest).
pub fn update_folder_build_now() -> bool {
    if !cfg!(windows) || cfg!(feature = "legacy") {
        return false;
    }
    std::thread::spawn(|| {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return false };
        rt.block_on(async {
            let Ok(client) = crate::http::build_client_with(&crate::http::current_proxy()) else { return false };
            match get_text(&client, YTDLP_SUMS_URL).await {
                Ok(sums) => yt_dlp_folder(&client, &sums).await,
                Err(_) => false,
            }
        })
    })
    .join()
    .unwrap_or(false)
}

/// Folder builds no longer in use (one still running stays until next time).
fn old_folder_builds() {
    let Some(root) = folder_build_root() else { return };
    let current = std::fs::read_to_string(root.join("current.txt")).unwrap_or_default();
    let Ok(list) = std::fs::read_dir(&root) else { return };
    for e in list.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if e.path().is_dir() && name != current.trim() {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// yt-dlp missing: get it. Present: `yt-dlp -U` (at most once a day).
async fn yt_dlp(client: &reqwest::Client, data_dir: &Path) {
    // the folder build (faster to start): fetched at once when missing,
    // else checked for a new one once a day; the one-file build stays as it
    // is, for when the folder build can't be had
    if cfg!(windows) && !cfg!(feature = "legacy") {
        let stamp = data_dir.join("yt-dlp-dir-checked");
        if ytdlp_folder_build().is_none() || due(&stamp, Duration::from_secs(20 * 3600)) {
            if let Ok(sums) = get_text(client, YTDLP_SUMS_URL).await {
                if yt_dlp_folder(client, &sums).await {
                    let _ = std::fs::write(&stamp, b"ok");
                    return;
                }
            }
        } else {
            return;
        }
    }
    #[cfg(feature = "legacy")]
    if bundled("yt-dlp").is_none() {
        legacy::fetch_ytdlp(client).await;
        return;
    }
    if bundled("yt-dlp").is_none() {
        // Linux and macOS: a copy from the distribution (apt, pacman,
        // homebrew...) does the job as well as ours
        if !cfg!(windows) && crate::mux::find_tool("yt-dlp").is_some() {
            return;
        }
        if let Some(dir) = writable_dir("yt-dlp") {
            let name = ytdlp_file();
            if let Ok(sums) = get_text(client, YTDLP_SUMS_URL).await {
                if let Some(sum) = sum_for(&sums, name) {
                    let url = format!("{YTDLP_RELEASE}{name}");
                    let target = dir.join(exe("yt-dlp"));
                    if download(client, &url, &target, &sum).await.is_ok() {
                        make_executable(&target);
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
        let mut cmd = ytdlp_base_command(&tool);
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
/// updated with the program; fetched again if it goes missing). Copies of the big public build that older
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

/// A small JavaScript engine (QuickJS-NG, MIT licence) for yt-dlp: some
/// YouTube videos (those made for children, for one) come only from YouTube
/// players whose links must be worked out by running YouTube's own script.
/// Fetched only when such a video is asked for, checked by its SHA-256.
const QJS_VERSION: &str = "v0.18.0";
const QJS_SHA256: &str = "ff03398d74ea6e6bd3a8e58058f7003225abcac229d7d57775b7aaae812a49c7";
const QJS_X86_SHA256: &str = "79aa85a8208596dc04333e99726c254d43b9e814803f2100ea593341f2c95b0e";

/// A JavaScript engine yt-dlp can use, as yt-dlp's `--js-runtimes` names
/// it with its place: Deno or Node.js the user has, else our QuickJS
/// (fetched now when it isn't there yet; blocking, a few seconds).
pub fn js_runtime() -> Option<(&'static str, PathBuf)> {
    if let Some(found) = installed_js_runtime() {
        return Some(found);
    }
    if let Some(q) = bundled("qjs") {
        return Some(("quickjs", q));
    }
    if !cfg!(windows) {
        return None;
    }
    let dir = writable_dir("qjs")?;
    // the 32-bit one in the 32-bit builds (it needs nothing Windows 7 lacks)
    let (file, sum) = if cfg!(target_arch = "x86") { ("qjs-windows-x86.exe", QJS_X86_SHA256) } else { ("qjs-windows-x86_64.exe", QJS_SHA256) };
    let url = format!("https://github.com/quickjs-ng/quickjs/releases/download/{QJS_VERSION}/{file}");
    let dest = dir.join(exe("qjs"));
    // its own small runtime on its own thread: callers may be on one of tokio's
    let got = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
        rt.block_on(async {
            let client = crate::http::build_client_with(&crate::http::current_proxy()).ok()?;
            download(&client, &url, &dest, sum).await.ok().map(|_| dest)
        })
    })
    .join()
    .ok()
    .flatten()?;
    Some(("quickjs", got))
}

/// Deno or Node.js installed on the computer (found where their own setups
/// put them; a program started from the desktop may not have them on its
/// PATH).
fn installed_js_runtime() -> Option<(&'static str, PathBuf)> {
    // the ones that still start on Windows 7 / 8 / 8.1 are too old for yt-dlp
    if cfg!(feature = "legacy") {
        return None;
    }
    let path: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
    let mut deno_dirs = path.clone();
    if let Some(h) = &home {
        deno_dirs.push(h.join(".deno").join("bin"));
    }
    if let Some(p) = deno_dirs.iter().map(|d| d.join(exe("deno"))).find(|p| p.is_file()) {
        return Some(("deno", p));
    }
    let mut node_dirs = path;
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        node_dirs.push(PathBuf::from(pf).join("nodejs"));
    }
    node_dirs.iter().map(|d| d.join(exe("node"))).find(|p| p.is_file()).map(|p| ("node", p))
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
    // the 32-bit one (Windows 7 / 8 / 8.1, 32-bit Windows 10) has its own
    // name in the release
    let file = if cfg!(target_arch = "x86") { "ffmpeg-x86.exe" } else { "ffmpeg.exe" };
    let urls = [
        format!("https://github.com/{repo}/releases/download/v{}/{file}", env!("CARGO_PKG_VERSION")),
        format!("https://github.com/{repo}/releases/latest/download/{file}"),
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
        for name in ["yt-dlp", "ffmpeg", "qjs"] {
            let _ = std::fs::remove_file(dir.join(exe(name)).with_extension("old"));
        }
    }
}

#[cfg(feature = "legacy")]
pub mod legacy;

/// Run `update_all` now and then every few hours while the app is open.
pub fn spawn_updater(rt: &tokio::runtime::Handle, client: reqwest::Client, data_dir: PathBuf) {
    rt.spawn(async move {
        // let the app start first (a few seconds only when a tool went
        // missing: it is fetched again at once)
        let missing = bundled("yt-dlp").is_none() || bundled("ffmpeg").is_none() || (cfg!(windows) && !cfg!(feature = "legacy") && ytdlp_folder_build().is_none());
        tokio::time::sleep(Duration::from_secs(if missing { 3 } else { 20 })).await;
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
