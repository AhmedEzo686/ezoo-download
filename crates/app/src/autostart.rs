//! "Start with Windows": a value under HKCU\...\Run that starts the app in
//! the background (next to the clock) when the user signs in.

#[cfg(windows)]
mod imp {
    use crate::winreg;

    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "EzooDownload";
    const OURS: &str = r"HKCU\Software\Ezoo\EzooDownload";

    pub fn is_enabled() -> bool {
        winreg::exists(KEY, Some(NAME))
    }

    /// Installed with the setup program (which asked about autostart itself).
    pub fn installed_by_setup() -> bool {
        winreg::exists(OURS, Some("Installed"))
    }

    /// What the setup program wrote for the first start ("Language",
    /// "Torrents", "Autostart").
    pub fn setup_value(name: &str) -> Option<String> {
        winreg::get(OURS, Some(name)).filter(|v| !v.trim().is_empty())
    }

    pub fn set(enabled: bool) -> bool {
        if enabled {
            let Ok(exe) = std::env::current_exe() else { return false };
            winreg::set(KEY, Some(NAME), &format!("\"{}\" --background", exe.display()))
        } else {
            winreg::delete_value(KEY, Some(NAME))
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
