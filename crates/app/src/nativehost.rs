//! The browser talks to the program through a small helper of ours that the
//! browser starts itself ("native messaging", ezoo-host.exe), not over the
//! network: nothing the browser does about web pages reaching this computer,
//! and no port another program (or Windows) keeps, can stop it.
//!
//! At start the program puts the helper in its data folder (a copy named by
//! its contents: the one a browser keeps open is never written over, so an
//! update never waits for the browser), says where the program is, and tells
//! the browsers about it (in the user's own part of the registry). The port
//! stays as it was, for the add-ins that don't use the helper (yet).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The helper's name for the browsers.
pub(crate) const NAME: &str = "com.ezoo.download";

/// Where each browser looks for helpers (the key's default value is the
/// helper's description file). Opera and Vivaldi read Chrome's.
#[cfg(windows)]
const CHROMIUM_KEYS: &[&str] = &[
    r"HKCU\Software\Google\Chrome\NativeMessagingHosts\com.ezoo.download",
    r"HKCU\Software\Chromium\NativeMessagingHosts\com.ezoo.download",
    r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.ezoo.download",
    r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.ezoo.download",
];
#[cfg(windows)]
const FIREFOX_KEY: &str = r"HKCU\Software\Mozilla\NativeMessagingHosts\com.ezoo.download";

struct State {
    dir: PathBuf,
    /// the add-ins of Chromium browsers that may start the helper
    ids: BTreeSet<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn host_file_name() -> &'static str {
    if cfg!(windows) {
        "ezoo-host.exe"
    } else {
        "ezoo-host"
    }
}

/// At start (on its own thread): the helper in place and the browsers told.
pub(crate) fn install(data_dir: &Path) {
    let Some(exe) = std::env::current_exe().ok() else { return };
    let Some(src) = exe.parent().map(|d| d.join(host_file_name())).filter(|p| p.is_file()) else {
        return; // a build without the helper: the add-in keeps the port
    };
    let Ok(bytes) = std::fs::read(&src) else { return };
    let dir = data_dir.join("host");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let host = match put_helper(&dir, &bytes) {
        Some(h) => h,
        None => return,
    };
    let app = serde_json::json!({
        "app": exe.display().to_string(),
        "settings": data_dir.join("settings.json").display().to_string(),
    });
    let _ = write_if_changed(&dir.join("app.json"), app.to_string().as_bytes());

    // the add-in folders of this program: their ids are known before the
    // add-in ever says it (Chrome names an add-in loaded from a folder after
    // the folder); ids the add-ins said themselves are kept too
    let mut ids: BTreeSet<String> = read_ids(&dir);
    for folder in crate::browser::extension_folders() {
        ids.insert(chrome_id_for_path(&folder));
    }
    let _ = write_if_changed(&dir.join("chromium.json"), chromium_manifest(&host, &ids).as_bytes());
    let _ = write_if_changed(&dir.join("firefox.json"), firefox_manifest(&host).as_bytes());
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(State { dir: dir.clone(), ids });

    #[cfg(windows)]
    {
        let chromium = dir.join("chromium.json").display().to_string();
        let firefox = dir.join("firefox.json").display().to_string();
        for key in CHROMIUM_KEYS {
            point(key, &chromium);
        }
        point(FIREFOX_KEY, &firefox);
    }
}

/// What the link test says about the helper: is it in place, and which
/// browsers know it. (browser, it knows the helper)
pub(crate) fn report() -> (bool, Vec<(&'static str, bool)>) {
    let st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(st) = st.as_ref() else { return (false, Vec::new()) };
    let helper = std::fs::read(st.dir.join("chromium.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["path"].as_str().map(PathBuf::from))
        .is_some_and(|p| p.is_file());
    #[cfg(windows)]
    let browsers = {
        let chromium = st.dir.join("chromium.json").display().to_string();
        let firefox = st.dir.join("firefox.json").display().to_string();
        let names = ["Chrome", "Chromium", "Edge", "Brave"];
        let mut out: Vec<(&'static str, bool)> =
            CHROMIUM_KEYS.iter().zip(names).map(|(k, n)| (n, crate::assoc::query(k, None).as_deref() == Some(chromium.as_str()))).collect();
        out.push(("Firefox", crate::assoc::query(FIREFOX_KEY, None).as_deref() == Some(firefox.as_str())));
        out
    };
    #[cfg(not(windows))]
    let browsers = Vec::new();
    (helper, browsers)
}

/// The key of a browser points at `file` (written only when it does not).
#[cfg(windows)]
fn point(key: &str, file: &str) {
    if crate::assoc::query(key, None).as_deref() != Some(file) {
        crate::assoc::reg(&["add", key, "/ve", "/d", file, "/f"]);
    }
}

/// The helper as a file named by its contents; older copies go (those a
/// browser still keeps open stay until next time).
fn put_helper(dir: &Path, bytes: &[u8]) -> Option<PathBuf> {
    use sha2::Digest;
    let sum = sha2::Sha256::digest(bytes);
    let tag: String = sum.iter().take(6).map(|b| format!("{b:02x}")).collect();
    let name = if cfg!(windows) { format!("ezoo-host-{tag}.exe") } else { format!("ezoo-host-{tag}") };
    let path = dir.join(&name);
    if std::fs::read(&path).map(|have| have != bytes).unwrap_or(true) {
        let tmp = dir.join(format!("{name}.new"));
        if std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("ezoo-host-") && n != name {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Some(path)
}

fn write_if_changed(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if std::fs::read(path).map(|have| have == data).unwrap_or(false) {
        return Ok(());
    }
    let tmp = path.with_extension("new");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

fn read_ids(dir: &Path) -> BTreeSet<String> {
    std::fs::read(dir.join("ids.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|i| is_chrome_id(i))
        .collect()
}

/// A Chromium add-in said who it is (each request): a new one may start the
/// helper from now on.
pub(crate) fn saw_addin(id: &str) {
    if !is_chrome_id(id) {
        return;
    }
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(st) = st.as_mut() else { return };
    if !st.ids.insert(id.to_string()) {
        return;
    }
    let ids: Vec<&String> = st.ids.iter().collect();
    let _ = write_if_changed(&st.dir.join("ids.json"), serde_json::to_string(&ids).unwrap_or_default().as_bytes());
    let host = std::fs::read(st.dir.join("chromium.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["path"].as_str().map(PathBuf::from));
    if let Some(host) = host {
        let _ = write_if_changed(&st.dir.join("chromium.json"), chromium_manifest(&host, &st.ids).as_bytes());
    }
}

fn is_chrome_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

fn chromium_manifest(host: &Path, ids: &BTreeSet<String>) -> String {
    let origins: Vec<String> = ids.iter().map(|i| format!("chrome-extension://{i}/")).collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "name": NAME,
        "description": "Ezoo Download",
        "path": host.display().to_string(),
        "type": "stdio",
        "allowed_origins": origins,
    }))
    .unwrap_or_default()
}

fn firefox_manifest(host: &Path) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "name": NAME,
        "description": "Ezoo Download",
        "path": host.display().to_string(),
        "type": "stdio",
        "allowed_extensions": [crate::browser::firefox_addon_id()],
    }))
    .unwrap_or_default()
}

/// The id Chrome gives an add-in loaded from `folder` (its full path, the
/// drive letter in capitals, hashed as Windows keeps the path: UTF-16).
pub(crate) fn chrome_id_for_path(folder: &Path) -> String {
    let mut s = folder.display().to_string();
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        let up = s[..1].to_ascii_uppercase();
        s.replace_range(..1, &up);
    }
    let bytes: Vec<u8> = if cfg!(windows) { s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect() } else { s.into_bytes() };
    chrome_id(&bytes)
}

fn chrome_id(input: &[u8]) -> String {
    use sha2::Digest;
    let sum = sha2::Sha256::digest(input);
    sum.iter().take(16).flat_map(|b| [b >> 4, b & 15]).map(|n| (b'a' + n) as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_as_chrome_makes_them() {
        // Chromium's own test value (crx_file id_util)
        assert_eq!(chrome_id(b"test"), "jpignaibiiemhngfjkcpokkamffknabf");
        assert!(is_chrome_id("jpignaibiiemhngfjkcpokkamffknabf"));
        assert!(!is_chrome_id("jpignaibiiemhngfjkcpokkamffknabz"));
        // the drive letter is the same either way
        assert_eq!(chrome_id_for_path(Path::new(r"c:\x\extension")), chrome_id_for_path(Path::new(r"C:\x\extension")));
    }

    #[test]
    fn the_helper_is_put_in_place() {
        let dir = std::env::temp_dir().join(format!("ezoo-host-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ezoo-host-old"), b"old").unwrap();
        let a = put_helper(&dir, b"one").unwrap();
        assert!(a.is_file());
        assert!(!dir.join("ezoo-host-old").exists(), "an older copy goes");
        let b = put_helper(&dir, b"two").unwrap();
        assert_ne!(a, b, "new contents, a new name (the old one may be open)");
        assert_eq!(put_helper(&dir, b"two").unwrap(), b);
        let ids: BTreeSet<String> = ["jpignaibiiemhngfjkcpokkamffknabf".to_string()].into();
        let m: serde_json::Value = serde_json::from_str(&chromium_manifest(&b, &ids)).unwrap();
        assert_eq!(m["allowed_origins"][0], "chrome-extension://jpignaibiiemhngfjkcpokkamffknabf/");
        assert_eq!(m["name"], NAME);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
