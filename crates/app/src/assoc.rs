//! "Make Ezoo Download the main program for torrents" (Options > Torrents):
//! .torrent files and magnet: links of Windows open in Ezoo Download, or
//! no more. The same keys the setup writes (its "Torrents" choice), in the
//! user's own part of the registry (no administrator needed).

/// On: Windows opens .torrent files and magnet: links with Ezoo Download.
/// Off: only what is Ezoo Download's own goes (another program's choice
/// stays as it is).
pub(crate) fn torrents(on: bool) {
    #[cfg(windows)]
    {
        let Ok(exe) = std::env::current_exe() else { return };
        let exe = exe.display().to_string();
        let open = format!("\"{exe}\" \"%1\"");
        let icon = format!("{exe},0");
        if on {
            for (key, name, data) in [
                (r"HKCU\Software\Classes\.torrent\OpenWithProgids", Some("EzooDownload.torrent"), ""),
                (r"HKCU\Software\Classes\.torrent", None, "EzooDownload.torrent"),
                (r"HKCU\Software\Classes\EzooDownload.torrent", None, "Torrent file"),
                (r"HKCU\Software\Classes\EzooDownload.torrent\DefaultIcon", None, icon.as_str()),
                (r"HKCU\Software\Classes\EzooDownload.torrent\shell\open\command", None, open.as_str()),
                (r"HKCU\Software\Classes\magnet", None, "URL:Magnet link"),
                (r"HKCU\Software\Classes\magnet", Some("URL Protocol"), ""),
                (r"HKCU\Software\Classes\magnet\DefaultIcon", None, icon.as_str()),
                (r"HKCU\Software\Classes\magnet\shell\open\command", None, open.as_str()),
            ] {
                let mut args = vec!["add", key];
                match name {
                    Some(n) => args.extend(["/v", n]),
                    None => args.push("/ve"),
                }
                args.extend(["/d", data, "/f"]);
                reg(&args);
            }
        } else {
            // ours only: a .torrent / magnet that another program took stays
            if query(r"HKCU\Software\Classes\.torrent", None).as_deref() == Some("EzooDownload.torrent") {
                reg(&["delete", r"HKCU\Software\Classes\.torrent", "/ve", "/f"]);
            }
            reg(&["delete", r"HKCU\Software\Classes\.torrent\OpenWithProgids", "/v", "EzooDownload.torrent", "/f"]);
            reg(&["delete", r"HKCU\Software\Classes\EzooDownload.torrent", "/f"]);
            if query(r"HKCU\Software\Classes\magnet\shell\open\command", None).is_some_and(|v| v.to_ascii_lowercase().contains("ezoodownload.exe")) {
                reg(&["delete", r"HKCU\Software\Classes\magnet", "/f"]);
            }
        }
        // Explorer learns of the change at once
        unsafe {
            use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
            SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null());
        }
    }
    #[cfg(not(windows))]
    let _ = on;
}

#[cfg(windows)]
fn reg(args: &[&str]) -> Option<std::process::Output> {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new(swiftget_engine::system::windows_tool("reg.exe")).args(args).creation_flags(NO_WINDOW).output().ok()
}

/// The value (None: the key's default) as `reg query` shows it.
#[cfg(windows)]
fn query(key: &str, name: Option<&str>) -> Option<String> {
    let mut args = vec!["query", key];
    match name {
        Some(n) => args.extend(["/v", n]),
        None => args.push("/ve"),
    }
    let out = reg(&args)?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    text.lines().find_map(|l| l.split_once("REG_SZ").map(|(_, v)| v.trim().to_string()))
}
