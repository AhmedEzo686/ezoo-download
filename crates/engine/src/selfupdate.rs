//! New versions of Ezoo Download itself.
//!
//! Every release on GitHub carries two files:
//!
//! * `EzooDownload-Setup.exe` – the installer
//! * `version.json`           – `{"version":"1.0.1","url":"…/EzooDownload-Setup.exe","sha256":"…"}`
//!
//! The app reads `version.json` of the latest release, and when the version
//! there is higher than its own it downloads the installer, checks that its
//! SHA-256 matches, and hands it back to the app to run.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const REPO: &str = "AhmedEzo686/ezoo-download";
pub const SETUP_NAME: &str = "EzooDownload-Setup.exe";

pub fn version_url() -> String {
    format!("https://github.com/{REPO}/releases/latest/download/version.json")
}

/// The running version.
pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Release {
    pub version: String,
    #[serde(default)]
    pub url: String,
    pub sha256: String,
}

impl Release {
    /// Where the installer is downloaded from. Only addresses of our own
    /// repository are accepted.
    pub fn setup_url(&self) -> String {
        let own = format!("https://github.com/{REPO}/");
        if self.url.starts_with(&own) {
            self.url.clone()
        } else {
            format!("https://github.com/{REPO}/releases/download/v{}/{SETUP_NAME}", self.version)
        }
    }
}

fn numbers(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .split('.')
        .map(|p| p.trim().parse().unwrap_or(0))
        .collect()
}

/// `remote` is a higher version than `local` ("1.0.10" > "1.0.9").
pub fn is_newer(remote: &str, local: &str) -> bool {
    let (mut a, mut b) = (numbers(remote), numbers(local));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

pub fn parse(text: &str) -> anyhow::Result<Release> {
    let r: Release = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    if r.sha256.len() != 64 || !r.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("version.json has no valid sha256");
    }
    if numbers(&r.version).iter().all(|n| *n == 0) {
        anyhow::bail!("version.json has no valid version");
    }
    Ok(r)
}

/// The latest release, if it is newer than this program.
pub async fn check(client: &reqwest::Client) -> anyhow::Result<Option<Release>> {
    let text = client
        .get(version_url())
        .header(reqwest::header::USER_AGENT, crate::config::DEFAULT_USER_AGENT)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let r = parse(&text)?;
    Ok(is_newer(&r.version, current()).then_some(r))
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn file_sha256(path: &Path) -> anyhow::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Download the installer of `r` into `dir` and check its SHA-256.
/// A copy that was already downloaded and checked is reused.
pub async fn download(client: &reqwest::Client, r: &Release, dir: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let dest = dir.join(format!("EzooDownload-Setup-{}.exe", r.version.trim_start_matches('v')));
    if dest.exists() {
        if file_sha256(&dest)?.eq_ignore_ascii_case(&r.sha256) {
            return Ok(dest);
        }
        let _ = std::fs::remove_file(&dest);
    }
    // older installers are not needed any more
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let mut resp = client
        .get(r.setup_url())
        .header(reqwest::header::USER_AGENT, crate::config::DEFAULT_USER_AGENT)
        .send()
        .await?
        .error_for_status()?;
    use std::io::Write;
    let tmp = dest.with_extension("download");
    let mut file = std::fs::File::create(&tmp)?;
    let mut h = Sha256::new();
    while let Some(chunk) = resp.chunk().await? {
        h.update(&chunk);
        file.write_all(&chunk)?;
    }
    file.flush()?;
    drop(file);
    let got: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !got.eq_ignore_ascii_case(&r.sha256) {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("the downloaded installer does not match version.json (sha256 {got})");
    }
    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}

/// Run the installer without any window. It closes this program, installs
/// the new version over it and starts it again.
pub fn run_installer(setup: &Path) -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new(setup);
    cmd.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/AUTOUPDATE=1"]);
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        cmd.arg(format!("/DIR={}", dir.display()));
    }
    cmd.spawn()?;
    Ok(())
}

/// Installed with the installer (not a portable copy): only then the
/// installer may replace the program.
pub fn installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("unins000.exe").exists()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(is_newer("v1.1", "1.0.9"));
        assert!(is_newer("1.0.10", "1.0.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0", "1.0.0"));
        assert!(!is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("garbage", "1.0.0"));
    }

    #[test]
    fn parse_and_url() {
        let sha = sha256_hex(b"abc");
        assert_eq!(sha, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let r = parse(&format!("\u{feff}{{\"version\":\"1.0.1\",\"sha256\":\"{sha}\"}}")).unwrap();
        assert_eq!(r.setup_url(), format!("https://github.com/{REPO}/releases/download/v1.0.1/{SETUP_NAME}"));
        let r2 = parse(&format!("{{\"version\":\"1.0.1\",\"url\":\"https://example.invalid/setup\",\"sha256\":\"{sha}\"}}")).unwrap();
        assert!(r2.setup_url().starts_with("https://github.com/"));
        assert!(parse("{\"version\":\"1.0.1\",\"sha256\":\"zz\"}").is_err());
    }
}
