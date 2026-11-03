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

/// Linux: an entry in ~/.config/autostart (the XDG way), read by GNOME, KDE,
/// Xfce and the other desktops when the user signs in.
#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::path::PathBuf;

    const FILE: &str = "ezoodownload.desktop";

    fn path() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(config.join("autostart").join(FILE))
    }

    pub fn is_enabled() -> bool {
        path().map(|p| p.exists()).unwrap_or(false)
    }

    /// Installed as a package (its own menu entry is there): the user asked
    /// for it once, so nothing is added behind their back.
    pub fn installed_by_setup() -> bool {
        ["/usr/share/applications", "/usr/local/share/applications"]
            .iter()
            .any(|d| std::path::Path::new(d).join("ezoodownload.desktop").exists())
    }

    /// A value for an Exec= line: quoted, so a folder with spaces works.
    fn exec_quote(path: &str) -> String {
        let mut out = String::from("\"");
        for c in path.chars() {
            if matches!(c, '"' | '\\' | '`' | '$') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        out
    }

    pub fn set(enabled: bool) -> bool {
        let Some(file) = path() else { return false };
        if !enabled {
            return !file.exists() || std::fs::remove_file(&file).is_ok();
        }
        let Ok(exe) = std::env::current_exe() else { return false };
        let exe = exe.display().to_string();
        if let Some(dir) = file.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return false;
            }
        }
        let entry = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Version=1.0\n\
             Name=Ezoo Download\n\
             Comment=Download manager\n\
             Exec={} --background\n\
             TryExec={}\n\
             Icon=ezoodownload\n\
             Terminal=false\n\
             StartupNotify=false\n\
             X-GNOME-Autostart-enabled=true\n",
            exec_quote(&exe),
            exe
        );
        std::fs::write(&file, entry).is_ok()
    }
}

/// macOS: a LaunchAgent in ~/Library/LaunchAgents, loaded at login.
#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};

    const LABEL: &str = "com.ezoo.EzooDownload";

    fn agent_path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
    }

    fn plist(executable: &Path) -> String {
        let escaped = executable
            .to_string_lossy()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;");
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{escaped}</string></array>
<key>RunAtLoad</key><true/>
<key>LimitLoadToSessionType</key><string>Aqua</string>
</dict></plist>
"#
        )
    }

    pub fn is_enabled() -> bool {
        agent_path().map(|p| p.exists()).unwrap_or(false)
    }

    pub fn installed_by_setup() -> bool {
        false
    }

    pub fn set(enabled: bool) -> bool {
        let Some(file) = agent_path() else { return false };
        if !enabled {
            return !file.exists() || std::fs::remove_file(&file).is_ok();
        }
        let Ok(exe) = std::env::current_exe() else { return false };
        if let Some(dir) = file.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return false;
            }
        }
        std::fs::write(&file, plist(&exe)).is_ok()
    }
}

#[cfg(not(any(windows, unix)))]
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
