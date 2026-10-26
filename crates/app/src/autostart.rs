//! "Start with Windows" / "start at sign-in": a value under HKCU\...\Run
//! (Windows) or a .desktop file in ~/.config/autostart (Linux) that starts
//! the app in the background (next to the clock) when the user signs in.

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;

    fn autostart_file() -> Option<PathBuf> {
        let dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(dir.join("autostart").join("ezoo-download.desktop"))
    }

    fn entry(exe: &std::path::Path) -> String {
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Version=1.0\n\
             Name=Ezoo Download\n\
             GenericName=Download Manager\n\
             Comment=Download files with multiple connections\n\
             Exec=\"{}\" --background\n\
             Icon=ezoo-download\n\
             Terminal=false\n\
             Categories=Network;FileTransfer;\n\
             X-GNOME-Autostart-enabled=true\n",
            exe.display()
        )
    }

    pub fn is_enabled() -> bool {
        autostart_file().is_some_and(|p| p.exists())
    }

    /// Installed by the .deb (which ships the desktop file but never asks
    /// about autostart), so like on Windows a copy from the setup does not
    /// turn autostart on by itself.
    pub fn installed_by_setup() -> bool {
        std::path::Path::new("/usr/share/applications/ezoo-download.desktop").exists()
    }

    pub fn set(enabled: bool) -> bool {
        let Some(path) = autostart_file() else { return false };
        if enabled {
            let Ok(exe) = std::env::current_exe() else { return false };
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            std::fs::write(&path, entry(&exe)).is_ok()
        } else {
            !is_enabled() || std::fs::remove_file(&path).is_ok()
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
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
}

pub use imp::{installed_by_setup, is_enabled, set};
