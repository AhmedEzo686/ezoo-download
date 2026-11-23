//! The helpers on Windows 7 / 8 / 8.1 (the legacy build).
//!
//! yt-dlp's own Windows programs need Windows 10. Here yt-dlp is its
//! one-file Python build (`yt-dlp`, the "zipimport" file of every yt-dlp
//! release, kept as `tools\yt-dlp.pyz`) run by a Python that comes with the
//! setup in `tools\python` (an embeddable Python built for Windows 7 and
//! newer). `yt-dlp.pyz -U` updates it like the program updates itself.
//!
//! That Python needs the Universal C Runtime of Windows. Windows 10 has it;
//! Windows 7 / 8 / 8.1 get it with Windows Update (KB2999226). When it is
//! missing the program offers Microsoft's own update for this Windows
//! (`ucrt_update`), checked by its SHA-256 before it is opened.

use super::{download, get_text, sum_for, tool_dirs, writable_dir, YTDLP_SUMS_URL};
use std::path::{Path, PathBuf};
use std::process::Command;

/// yt-dlp's one-file Python build of the latest release.
const YTDLP_PYZ_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp";
/// its name in yt-dlp's list of SHA-256
const YTDLP_PYZ_NAME: &str = "yt-dlp";
/// its name in our tools folder
const PYZ: &str = "yt-dlp.pyz";

/// Our Python (`tools\python\python.exe`).
pub fn python() -> Option<PathBuf> {
    tool_dirs().into_iter().map(|d| d.join("python").join("python.exe")).find(|p| p.is_file())
}

/// yt-dlp (its Python build) when it can run: the file and our Python.
pub fn ytdlp() -> Option<PathBuf> {
    // without the C runtime Python can't start (and Windows would show
    // its own "a .dll is missing" box): see ucrt_update
    if ucrt_missing() {
        return None;
    }
    python()?;
    tool_dirs().into_iter().map(|d| d.join(PYZ)).find(|p| p.is_file())
}

/// `python -X utf8 yt-dlp.pyz` for yt-dlp's Python build. UTF-8 mode: the
/// program reads yt-dlp's answers (titles in every language) as UTF-8, as
/// yt-dlp.exe writes them.
pub fn command(tool: &Path) -> Option<Command> {
    if tool.extension().is_none_or(|e| !e.eq_ignore_ascii_case("pyz")) {
        return None;
    }
    let py = python()?;
    let mut cmd = Command::new(py);
    cmd.args(["-X", "utf8"]).arg(tool);
    Some(cmd)
}

/// yt-dlp is missing: its newest Python build, checked against yt-dlp's
/// own list of SHA-256.
pub async fn fetch_ytdlp(client: &reqwest::Client) {
    let Some(dir) = writable_dir("yt-dlp") else { return };
    let Ok(sums) = get_text(client, YTDLP_SUMS_URL).await else { return };
    let Some(sum) = sum_for(&sums, YTDLP_PYZ_NAME) else { return };
    let _ = download(client, YTDLP_PYZ_URL, &dir.join(PYZ), &sum).await;
}

/// The Universal C Runtime is not on this Windows (Python can't start).
pub fn ucrt_missing() -> bool {
    if !cfg!(windows) {
        return false;
    }
    // a 32-bit program asking for System32 is given SysWOW64 on 64-bit
    // Windows: the 32-bit runtime, the one our Python needs
    let Some(root) = std::env::var_os("SystemRoot") else { return false };
    !Path::new(&root).join("System32").join("ucrtbase.dll").is_file()
}

/// Microsoft's update that brings the Universal C Runtime (KB2999226) for
/// this Windows: (file name, address, SHA-256). None on a Windows that has
/// it built in (10 and newer) or that we don't know.
fn ucrt_package() -> Option<(&'static str, &'static str, &'static str)> {
    let (major, minor) = windows_version()?;
    let x64 = windows_is_64bit();
    Some(match (major, minor, x64) {
        (6, 1, false) => (
            "Windows6.1-KB2999226-x86.msu",
            "https://download.microsoft.com/download/4/F/E/4FE73868-5EDD-4B47-8B33-CE1BB7B2B16A/Windows6.1-KB2999226-x86.msu",
            "909e76c81ef0eb176144b253ddffe7a8fdfacebfaa15e97def003d2262fbf084",
        ),
        (6, 1, true) => (
            "Windows6.1-KB2999226-x64.msu",
            "https://download.microsoft.com/download/1/1/5/11565A9A-EA09-4F0A-A57E-520D5D138140/Windows6.1-KB2999226-x64.msu",
            "43234d2986ca9b0de75d5183977964d161a8395c3396279ddfc9b20698e5bc34",
        ),
        (6, 2, false) => (
            "Windows8-RT-KB2999226-x86.msu",
            "https://download.microsoft.com/download/1/E/8/1E8AFE90-5217-464D-9292-7D0B95A56CE4/Windows8-RT-KB2999226-x86.msu",
            "0f36750fbb06fee23131f68b4d0943841eed24730ec1d5d77dedc41d359be88d",
        ),
        (6, 2, true) => (
            "Windows8-RT-KB2999226-x64.msu",
            "https://download.microsoft.com/download/A/C/1/AC15393F-A6E6-469B-B222-C44B3BB6ECCC/Windows8-RT-KB2999226-x64.msu",
            "50cae25da33fa950222d1a803e42567291eb7feb087fa119b1c97fe9d41cd9f8",
        ),
        (6, 3, false) => (
            "Windows8.1-KB2999226-x86.msu",
            "https://download.microsoft.com/download/E/4/6/E4694323-8290-4A08-82DB-81F2EB9452C2/Windows8.1-KB2999226-x86.msu",
            "b83251219c5390536b02bebaf5e43a6f13381ce1db43e76483bce07c4bcf877b",
        ),
        (6, 3, true) => (
            "Windows8.1-KB2999226-x64.msu",
            "https://download.microsoft.com/download/9/6/F/96FD0525-3DDF-423D-8845-5F92F4A6883E/Windows8.1-KB2999226-x64.msu",
            "9f707096c7d279ed4bc2a40ba695efac69c20406e0ca97e2b3e08443c6381d15",
        ),
        _ => return None,
    })
}

/// Fetch Microsoft's update with the Universal C Runtime for this Windows
/// into `dir`; the caller opens it (Windows asks to install it).
pub async fn ucrt_update(client: &reqwest::Client, dir: &Path) -> anyhow::Result<PathBuf> {
    let (name, url, sum) = ucrt_package().ok_or_else(|| anyhow::anyhow!("no update is known for this Windows"))?;
    std::fs::create_dir_all(dir)?;
    let dest = dir.join(name);
    download(client, url, &dest, sum).await?;
    Ok(dest)
}

/// Windows 10 or 11 (put on a computer that had this build): it updates to
/// the build for them.
pub fn windows_10_or_newer() -> bool {
    windows_version().is_some_and(|(major, _)| major >= 10)
}

/// Windows' own version (6.1 = 7, 6.2 = 8, 6.3 = 8.1, 10.0 = 10 / 11). The
/// program's manifest names all of them, so Windows tells the truth.
#[cfg(windows)]
fn windows_version() -> Option<(u32, u32)> {
    use windows_sys::Win32::System::SystemInformation::{GetVersionExW, OSVERSIONINFOW};
    let mut v: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
    v.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
    (unsafe { GetVersionExW(&mut v) } != 0).then_some((v.dwMajorVersion, v.dwMinorVersion))
}
#[cfg(not(windows))]
fn windows_version() -> Option<(u32, u32)> {
    None
}

/// 64-bit Windows (this program is 32-bit everywhere).
#[cfg(windows)]
fn windows_is_64bit() -> bool {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process};
    let mut wow = 0;
    unsafe { IsWow64Process(GetCurrentProcess(), &mut wow) != 0 && wow != 0 }
}
#[cfg(not(windows))]
fn windows_is_64bit() -> bool {
    false
}
