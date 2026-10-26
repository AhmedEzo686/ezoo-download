//! Platform services: disk space, file system type, power actions, file
//! dates and the virus checker.

use crate::config::AfterDone;
use std::path::Path;

/// Free bytes on the disk that holds `dir` (None when unknown).
pub fn free_space(dir: &Path) -> Option<u64> {
    imp::free_space(dir)
}

/// "NTFS", "FAT32", "exFAT"... of the disk that holds `dir`.
pub fn file_system(dir: &Path) -> Option<String> {
    imp::file_system(dir)
}

/// Is there room for `needed` more bytes in `dir`? Returns a message if not.
pub fn check_room(dir: &Path, needed: u64, total: Option<u64>) -> Result<(), String> {
    let dir = existing_parent(dir);
    if let Some(free) = free_space(&dir) {
        // keep a little air for the file system
        if needed > 0 && free < needed.saturating_add(16 * 1024 * 1024) {
            return Err(format!(
                "Not enough free space on the disk: the file needs {} more, only {} free.",
                crate::fmt_size(needed),
                crate::fmt_size(free)
            ));
        }
    }
    if let Some(total) = total {
        if total >= 4 * 1024 * 1024 * 1024 {
            if let Some(fs) = file_system(&dir) {
                if fs.eq_ignore_ascii_case("FAT32") || fs.eq_ignore_ascii_case("FAT") || fs.eq_ignore_ascii_case("msdos") {
                    #[cfg(target_os = "macos")]
                    return Err(format!(
                        "This disk uses {fs}, which can't hold files bigger than 4 GB. Save the file to an APFS or exFAT disk."
                    ));
                    #[cfg(not(target_os = "macos"))]
                    return Err(format!(
                        "This disk uses {fs}, which can't hold files bigger than 4 GB. Save the file to an NTFS or exFAT disk."
                    ));
                }
            }
        }
    }
    Ok(())
}

fn existing_parent(dir: &Path) -> std::path::PathBuf {
    let mut d = dir.to_path_buf();
    while !d.exists() {
        match d.parent() {
            Some(p) if p != d => d = p.to_path_buf(),
            _ => break,
        }
    }
    d
}

/// Give a finished file the server's date.
pub fn set_file_time(path: &Path, t: chrono::DateTime<chrono::Utc>) {
    let ft = filetime::FileTime::from_unix_time(t.timestamp(), 0);
    let _ = filetime::set_file_mtime(path, ft);
}

/// Run the virus checker on a finished file: `args` with "[File]" replaced.
/// A program that comes with Windows, by its full path (System32), so a file
/// with the same name somewhere else is never started instead.
pub fn windows_tool(name: &str) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("windir"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
        if name.eq_ignore_ascii_case("explorer.exe") {
            return root.join(name);
        }
        root.join("System32").join(name)
    }
    #[cfg(not(windows))]
    {
        std::path::PathBuf::from(name)
    }
}

/// The address without a user name / password and without the "#..." part,
/// for the "downloaded from" note of a file. None when it is not a web address.
fn plain_web_url(u: &str) -> Option<String> {
    let mut url = url::Url::parse(u.trim()).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_fragment(None);
    Some(url.to_string())
}

/// What Windows keeps in `<file>:Zone.Identifier` for a file from the
/// internet (the "Mark of the Web", as browsers write it).
pub fn zone_identifier(host_url: &str, referrer: Option<&str>) -> String {
    let mut out = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if let Some(r) = referrer.and_then(plain_web_url) {
        out.push_str(&format!("ReferrerUrl={r}\r\n"));
    }
    if let Some(h) = plain_web_url(host_url) {
        out.push_str(&format!("HostUrl={h}\r\n"));
    }
    out
}

/// Mark a downloaded file as coming from the internet, like browsers do:
/// Windows SmartScreen and Office then warn before it is opened or run.
/// Best effort (a disk without NTFS streams, e.g. FAT32, has no place for it).
pub fn mark_from_internet(file: &Path, host_url: &str, referrer: Option<&str>) {
    #[cfg(windows)]
    {
        let mut stream = file.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = std::fs::write(std::path::PathBuf::from(stream), zone_identifier(host_url, referrer));
    }
    #[cfg(not(windows))]
    {
        let _ = (file, host_url, referrer);
    }
}

pub fn virus_scan(program: &str, args: &str, file: &Path) {
    let program = program.trim().trim_matches('"');
    if program.is_empty() {
        return;
    }
    let file = file.display().to_string();
    let args = if args.contains("[File]") {
        args.replace("[File]", &format!("\"{file}\""))
    } else {
        format!("{args} \"{file}\"")
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(split_args(&args));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = cmd.spawn();
}

/// Split a command line into arguments ("quoted parts" stay together).
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Shut down, sleep or hibernate now (after the countdown).
pub fn power(action: AfterDone, force: bool) {
    imp::power(action, force)
}

#[cfg(windows)]
mod imp {
    use super::AfterDone;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetVolumeInformationW, GetVolumePathNameW};

    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn free_space(dir: &Path) -> Option<u64> {
        let w = wide(dir);
        let mut free: u64 = 0;
        let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
        (ok != 0).then_some(free)
    }

    pub fn file_system(dir: &Path) -> Option<String> {
        let w = wide(dir);
        let mut root = [0u16; 300];
        if unsafe { GetVolumePathNameW(w.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0 {
            return None;
        }
        let mut name = [0u16; 64];
        let ok = unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                name.as_mut_ptr(),
                name.len() as u32,
            )
        };
        if ok == 0 {
            return None;
        }
        let len = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        Some(String::from_utf16_lossy(&name[..len]))
    }

    pub fn power(action: AfterDone, force: bool) {
        use std::os::windows::process::CommandExt;
        let run = |args: &[&str]| {
            let _ = std::process::Command::new(super::windows_tool("shutdown.exe"))
                .args(args)
                .creation_flags(0x0800_0000)
                .spawn();
        };
        match action {
            AfterDone::Shutdown => {
                if force {
                    run(&["/s", "/f", "/t", "0"]);
                } else {
                    run(&["/s", "/t", "0"]);
                }
            }
            AfterDone::Hibernate => run(&["/h"]),
            AfterDone::Sleep => unsafe {
                windows_sys::Win32::System::Power::SetSuspendState(0, u8::from(force), 0);
            },
            _ => {}
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::AfterDone;
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    use std::process::Command;

    fn stat(dir: &Path) -> Option<libc::statfs> {
        let path = CString::new(dir.as_os_str().as_bytes()).ok()?;
        let mut out = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // The C string remains alive for the call; statfs initializes the
        // complete output structure only when it returns zero.
        if unsafe { libc::statfs(path.as_ptr(), out.as_mut_ptr()) } != 0 {
            return None;
        }
        Some(unsafe { out.assume_init() })
    }

    pub fn free_space(dir: &Path) -> Option<u64> {
        let fs = stat(dir)?;
        fs.f_bavail.checked_mul(u64::from(fs.f_bsize))
    }

    pub fn file_system(dir: &Path) -> Option<String> {
        let fs = stat(dir)?;
        // macOS returns the filesystem name as a NUL-terminated array.
        Some(unsafe { CStr::from_ptr(fs.f_fstypename.as_ptr()) }.to_string_lossy().into_owned())
    }

    pub fn power(action: AfterDone, _force: bool) {
        // Never change the Mac's hibernation settings or force-close apps.
        // macOS controls safe sleep itself; explicit hibernation is unsupported.
        match action {
            AfterDone::Sleep => {
                let _ = Command::new("/usr/bin/pmset").arg("sleepnow").spawn();
            }
            AfterDone::Shutdown => {
                let _ = Command::new("/usr/bin/osascript")
                    .args(["-e", "tell application \"System Events\" to shut down"])
                    .spawn();
            }
            _ => {}
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::AfterDone;
    use std::path::Path;
    pub fn free_space(_dir: &Path) -> Option<u64> {
        None
    }
    pub fn file_system(_dir: &Path) -> Option<String> {
        None
    }
    pub fn power(_action: AfterDone, _force: bool) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    #[test]
    fn mac_disk_information_and_room_check() {
        let dir = std::env::temp_dir();
        assert!(free_space(&dir).unwrap() > 0);
        assert!(!file_system(&dir).unwrap().is_empty());
        let missing = dir.join(format!("ezoo-no-disk-{}", std::process::id())).join("file");
        assert!(free_space(&missing).is_none());
        assert!(check_room(&missing, 1, Some(1)).is_ok());
        assert!(check_room(&dir, u64::MAX, None).is_err());
    }

    #[test]
    fn args() {
        assert_eq!(split_args(r#"/scan "C:\a b\x.zip" -q"#), vec!["/scan", r"C:\a b\x.zip", "-q"]);
        assert_eq!(split_args(""), Vec::<String>::new());
    }

    #[test]
    fn mark_of_the_web() {
        let z = zone_identifier("https://user:pw@cdn.example.com/a.exe?x=1#frag", Some("https://example.com/page"));
        assert!(z.starts_with("[ZoneTransfer]\r\nZoneId=3\r\n"));
        assert!(z.contains("HostUrl=https://cdn.example.com/a.exe?x=1\r\n"));
        assert!(z.contains("ReferrerUrl=https://example.com/page\r\n"));
        assert!(!z.contains("pw") && !z.contains("frag"));
        // only web addresses are written down
        let z = zone_identifier("file:///C:/x.exe", Some("javascript:alert(1)"));
        assert_eq!(z, "[ZoneTransfer]\r\nZoneId=3\r\n");
    }
}
