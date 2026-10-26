//! Start at sign-in: the Windows Run key or an opt-in macOS LaunchAgent.

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

#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};

    const LABEL: &str = "com.ezoo.EzooDownload";

    fn agent_path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
    }

    fn plist(executable: &Path) -> String {
        let escaped = executable.to_string_lossy()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\"', "&quot;")
            .replace('\'', "&apos;");
        // No --background: macOS currently has no tray icon to reopen a hidden window.
        format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{escaped}</string></array>
<key>RunAtLoad</key><true/>
<key>LimitLoadToSessionType</key><string>Aqua</string>
</dict></plist>
"#)
    }

    pub fn is_enabled() -> bool {
        agent_path().is_some_and(|path| path.is_file())
    }

    pub fn installed_by_setup() -> bool {
        false
    }

    pub fn set(enabled: bool) -> bool {
        let Some(path) = agent_path() else { return false };
        if !enabled {
            return !path.exists() || std::fs::remove_file(path).is_ok();
        }
        let Ok(executable) = std::env::current_exe() else { return false };
        let Some(parent) = path.parent() else { return false };
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
        // launchd reads ~/Library/LaunchAgents at the next sign-in. Writing
        // through a temporary file avoids leaving a partial property list.
        let staged = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = std::fs::write(&staged, plist(&executable)).and_then(|_| std::fs::rename(&staged, &path));
        if result.is_err() {
            let _ = std::fs::remove_file(staged);
        }
        result.is_ok()
    }

    #[cfg(test)]
    mod tests {
        use super::plist;
        use std::path::Path;

        #[test]
        fn launch_agent_escapes_paths_and_starts_a_visible_window() {
            let value = plist(Path::new("/Applications/Ezoo & <Friends> \"quoted\".app/Contents/MacOS/EzooDownload"));
            assert!(value.contains("Ezoo &amp; &lt;Friends&gt; &quot;quoted&quot;.app"));
            assert!(value.contains("<key>RunAtLoad</key><true/>"));
            assert!(!value.contains("--background"));
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
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
