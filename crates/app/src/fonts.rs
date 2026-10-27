//! The fonts of the interface, put in place when the program starts:
//! Arabic shows in **InstaArabi**, the Latin texts in **Times New Roman**
//! (a metric copy of it). Both travel inside the program, so they also
//! work on computers where neither font is installed.

use std::path::PathBuf;

/// Arabic text: the InstaArabi font the project ships with.
pub(crate) const INSTAARABI: &[u8] = include_bytes!("../ui/fonts/InstaArabi-Regular.ttf");
/// Latin text: Times New Roman by metrics (see ui/fonts/FONTS-LICENSE.txt).
pub(crate) const TIMES: &[u8] = include_bytes!("../ui/fonts/TimesNewRoman-Regular.ttf");
pub(crate) const TIMES_BOLD: &[u8] = include_bytes!("../ui/fonts/TimesNewRoman-Bold.ttf");

/// Write every font to the user's font folder (the first time only), so
/// the program and the system dialogs all see it from the next start on.
/// Runs before any window opens, while the font list is still collected.
pub(crate) fn install() {
    let fonts: [(&str, &[u8]); 3] = [
        ("InstaArabi-Regular.ttf", INSTAARABI),
        ("TimesNewRoman-Regular.ttf", TIMES),
        ("TimesNewRoman-Bold.ttf", TIMES_BOLD),
    ];
    for (name, bytes) in fonts {
        let _ = write_font(name, bytes);
    }
}

#[cfg(target_os = "linux")]
fn write_font(name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no home folder"))?;
    let dir = base.join("fonts").join("Ezoo");
    std::fs::create_dir_all(&dir)?;
    let f = dir.join(name);
    if f.exists() {
        return Ok(());
    }
    let tmp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &f)
}

#[cfg(windows)]
fn write_font(name: &str, bytes: &[u8]) -> std::io::Result<()> {
    // per-user fonts: a file in %LOCALAPPDATA%\Microsoft\Windows\Fonts and
    // one registry value, exactly what "Install for me" in Explorer does
    use std::os::windows::ffi::OsStrExt;
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no LOCALAPPDATA"))?;
    let dir = base.join("Microsoft").join("Windows").join("Fonts");
    std::fs::create_dir_all(&dir)?;
    let f = dir.join(name);
    if !f.exists() {
        std::fs::write(&f, bytes)?;
    }
    // "InstaArabi-Regular.ttf" -> "InstaArabi (TrueType)"
    let family = name.split('-').next().unwrap_or(name).trim_end_matches(".ttf");
    let label = wide(format!("{family} (TrueType)"));
    let key = wide(r"Software\Microsoft\Windows NT\CurrentVersion\Fonts".to_string());
    let path = wide(f.display().to_string());
    unsafe {
        let mut hkey = std::ptr::null_mut();
        let open = windows_sys::Win32::System::Registry::RegCreateKeyExW(
            windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
            key.as_ptr(),
            0,
            std::ptr::null(),
            0,
            windows_sys::Win32::System::Registry::KEY_SET_VALUE,
            std::ptr::null(),
            &mut hkey,
            std::ptr::null_mut(),
        );
        if open == 0 {
            windows_sys::Win32::System::Registry::RegSetValueExW(
                hkey,
                label.as_ptr(),
                0,
                windows_sys::Win32::System::Registry::REG_SZ,
                path.as_ptr() as *const u8,
                (path.len() * 2) as u32,
            );
            windows_sys::Win32::System::Registry::RegCloseKey(hkey);
        }
    }
    Ok(())
}

#[cfg(windows)]
fn wide(s: String) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(&s).encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn write_font(_name: &str, _bytes: &[u8]) -> std::io::Result<()> {
    Ok(())
}
