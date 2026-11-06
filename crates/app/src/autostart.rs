//! "Start with Windows": a value under HKCU\...\Run that starts the app in
//! the background (next to the clock) when the user signs in.

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "EzooDownload";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn reg(args: &[&str]) -> bool {
        Command::new(swiftget_engine::system::windows_tool("reg.exe"))
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn is_enabled() -> bool {
        reg(&["query", KEY, "/v", NAME])
    }

    /// Installed with the setup program (which asked about autostart itself).
    pub fn installed_by_setup() -> bool {
        reg(&["query", r"HKCU\Software\Ezoo\EzooDownload", "/v", "Installed"])
    }

    /// What the setup program wrote for the first start ("Language",
    /// "Torrents").
    pub fn setup_value(name: &str) -> Option<String> {
        let out = Command::new(swiftget_engine::system::windows_tool("reg.exe"))
            .args(["query", r"HKCU\Software\Ezoo\EzooDownload", "/v", name])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()
            .filter(|o| o.status.success())?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        text.lines().find_map(|l| l.split_once("REG_SZ").map(|(_, v)| v.trim().to_string())).filter(|v| !v.is_empty())
    }

    pub fn set(enabled: bool) -> bool {
        if enabled {
            let Ok(exe) = std::env::current_exe() else { return false };
            let value = format!("\"{}\" --background", exe.display());
            reg(&["add", KEY, "/v", NAME, "/t", "REG_SZ", "/d", &value, "/f"])
        } else {
            !is_enabled() || reg(&["delete", KEY, "/v", NAME, "/f"])
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn is_enabled() -> bool {
        false
    }
    pub fn set(_enabled: bool) -> bool {
        false
    }
    pub fn installed_by_setup() -> bool {
        false
    }
    pub fn setup_value(_name: &str) -> Option<String> {
        None
    }
}

pub use imp::{installed_by_setup, is_enabled, set, setup_value};
