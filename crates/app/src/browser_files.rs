//! A stable, app-owned location for browser add-ins, outside the app bundle.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MARKER: &str = ".ezoo-managed";
const OWNER: &str = "Ezoo Download browser extension";
static PREPARING: std::sync::Mutex<()> = std::sync::Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Marker {
    owner: String,
    schema: u8,
    flavor: String,
    version: String,
}

fn flavor(firefox: bool) -> &'static str {
    if firefox { "firefox" } else { "chromium" }
}

fn destination(data_dir: &Path, firefox: bool) -> PathBuf {
    data_dir.join("Browser Extensions").join(if firefox {
        "Ezoo Download Firefox Extension"
    } else {
        "Ezoo Download Extension"
    })
}

fn is_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}

fn private_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => return Ok(()),
        Ok(_) => bail!("Not a regular directory: {}", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        private_dir(parent)?;
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).with_context(|| format!("Could not create {}", path.display()))
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(parent: &Path, label: &str) -> Result<Self> {
        for _ in 0..10 {
            let count = NEXT.fetch_add(1, Ordering::Relaxed);
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
            let path = parent.join(format!(".ezoo-{label}-{}-{nanos:x}-{count}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        bail!("Could not create an extension staging directory")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn small_json(path: &Path) -> Result<serde_json::Value> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        bail!("Not a regular extension file: {}", path.display());
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        bail!("Extension metadata is too large");
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn valid_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    (1..=4).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok())
}

fn read_marker(target: &Path, firefox: bool) -> Result<Marker> {
    if !is_directory(target) {
        bail!("The extension destination is not a regular directory");
    }
    let marker: Marker = serde_json::from_value(small_json(&target.join(MARKER))?)?;
    if marker.owner != OWNER || marker.schema != 1 || marker.flavor != flavor(firefox) || !valid_version(&marker.version) {
        bail!("The extension folder is not managed by Ezoo Download");
    }
    Ok(marker)
}

/// Only return a copy which this app previously prepared. Unknown folders
/// must never be overwritten automatically during app startup.
pub(crate) fn prepared_dir(data_dir: &Path, firefox: bool) -> Option<PathBuf> {
    let target = destination(data_dir, firefox);
    if !is_directory(data_dir) || !is_directory(target.parent()?) {
        return None;
    }
    read_marker(&target, firefox).ok()?;
    Some(target)
}

// Directories have None as their size; files have Some(length). Dotfiles and
// development-only resources are deliberately never part of the installed copy.
fn inventory(root: &Path, source: bool) -> Result<BTreeMap<PathBuf, Option<u64>>> {
    fn walk(root: &Path, relative: &Path, entries: &mut BTreeMap<PathBuf, Option<u64>>, source: bool) -> Result<()> {
        for entry in fs::read_dir(root.join(relative))? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                bail!("Symbolic links are not allowed in browser extension files");
            }
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') || name == "test" || name == "manifest.firefox.json" || (source && name == "app-connection.json") {
                continue;
            }
            let path = relative.join(name);
            if kind.is_dir() {
                entries.insert(path.clone(), None);
                walk(root, &path, entries, source)?;
            } else if kind.is_file() {
                entries.insert(path, Some(entry.metadata()?.len()));
            } else {
                bail!("Unsupported browser extension file: {}", entry.path().display());
            }
        }
        Ok(())
    }
    if !is_directory(root) {
        bail!("The extension source is not a regular directory");
    }
    let mut entries = BTreeMap::new();
    walk(root, Path::new(""), &mut entries, source)?;
    Ok(entries)
}

fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    let mut file = private_file(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn files_equal(left: &Path, right: &Path) -> Result<bool> {
    let (mut left, mut right) = (File::open(left)?, File::open(right)?);
    let (mut a, mut b) = ([0u8; 8192], [0u8; 8192]);
    loop {
        let n = left.read(&mut a)?;
        let m = right.read(&mut b)?;
        if n != m || a[..n] != b[..m] { return Ok(false); }
        if n == 0 { return Ok(true); }
    }
}

fn replace(stage: &Path, target: &Path, parent: &Path, existing: bool) -> Result<()> {
    if !existing {
        fs::rename(stage, target)?;
        return Ok(());
    }
    let backup = Scratch::new(parent, "backup")?;
    let previous = backup.0.join("previous");
    fs::rename(target, &previous)?;
    if let Err(error) = fs::rename(stage, target) {
        if let Err(rollback) = fs::rename(&previous, target) {
            // Leave the previous installation recoverable if the filesystem
            // also refuses the rollback; Drop must not erase the only copy.
            let retained = backup.0.clone();
            std::mem::forget(backup);
            bail!("Extension update failed ({error}); restore failed ({rollback}). Previous copy preserved at {}", retained.display());
        }
        return Err(error.into());
    }
    Ok(())
}

/// Stage the bundled extension, then replace its managed copy as a complete
/// directory. Neither the app bundle nor pre-existing unmarked folders change.
pub(crate) fn prepare(source: &Path, data_dir: &Path, firefox: bool, port: u16) -> Result<PathBuf> {
    let _lock = PREPARING.lock().unwrap_or_else(|p| p.into_inner());
    if port == 0 { bail!("The browser connection port must be between 1 and 65535"); }
    let entries = inventory(source, true)?;
    let manifest = small_json(&source.join("manifest.json"))?;
    let version = manifest["version"].as_str().filter(|v| valid_version(v)).context("Invalid extension version")?;
    let source_firefox = manifest.get("browser_specific_settings").and_then(|v| v.get("gecko")).is_some();
    if manifest["manifest_version"] != 3 || source_firefox != firefox {
        bail!("The bundled extension does not match this browser");
    }
    let marker = Marker { owner: OWNER.into(), schema: 1, flavor: flavor(firefox).into(), version: version.into() };
    let target = destination(data_dir, firefox);
    let parent = target.parent().expect("extension destination has a parent");
    private_dir(data_dir)?;
    private_dir(parent)?;
    let previous = match fs::symlink_metadata(&target) {
        Ok(_) => Some(read_marker(&target, firefox).context("Refusing to replace an unknown extension folder")?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let stage = Scratch::new(parent, "stage")?;
    for (relative, size) in &entries {
        let path = stage.0.join(relative);
        if size.is_none() {
            private_dir(&path)?;
        } else {
            let mut input = File::open(source.join(relative))?;
            let mut output = private_file(&path)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
        }
    }
    write_private(&stage.0.join(MARKER), &serde_json::to_vec(&marker)?)?;
    write_private(&stage.0.join("app-connection.json"), &serde_json::to_vec(&serde_json::json!({"port": port}))?)?;
    let staged_entries = inventory(&stage.0, false)?;
    if previous.as_ref() == Some(&marker) && inventory(&target, false)? == staged_entries {
        let mut equal = true;
        for (relative, size) in &staged_entries {
            if size.is_some() && !files_equal(&stage.0.join(relative), &target.join(relative))? {
                equal = false;
                break;
            }
        }
        if equal { return Ok(target); }
    }
    replace(&stage.0, &target, parent, previous.is_some())?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Scratch, PathBuf, PathBuf) {
        let root = Scratch::new(&std::env::temp_dir(), "browser-test").unwrap();
        let source = root.0.join("source");
        let data = root.0.join("data");
        fs::create_dir(&source).unwrap();
        fs::create_dir(source.join("icons")).unwrap();
        fs::write(source.join("manifest.json"), br#"{"manifest_version":3,"version":"1.0.6"}"#).unwrap();
        fs::write(source.join("background.js"), b"original script").unwrap();
        fs::write(source.join("icons/icon.png"), b"synthetic icon").unwrap();
        (root, source, data)
    }

    #[test]
    fn stable_copy_updates_without_changing_the_source_or_identical_destination() {
        let (_root, source, data) = fixture();
        fs::create_dir(source.join("test")).unwrap();
        fs::write(source.join("test/development.js"), b"not installed").unwrap();
        fs::write(source.join(".private"), b"not installed").unwrap();
        fs::write(source.join("manifest.firefox.json"), b"not installed").unwrap();
        fs::write(source.join("app-connection.json"), b"{\"port\":1}").unwrap();
        let target = prepare(&source, &data, false, 9614).unwrap();
        assert_eq!(prepared_dir(&data, false), Some(target.clone()));
        assert_eq!(prepared_dir(&data, true), None);
        assert_eq!(target, data.join("Browser Extensions/Ezoo Download Extension"));
        assert!(!target.join("test").exists());
        assert!(!target.join(".private").exists());
        assert!(!target.join("manifest.firefox.json").exists());
        assert_eq!(small_json(&target.join("app-connection.json")).unwrap()["port"], 9614);
        assert_eq!(small_json(&source.join("app-connection.json")).unwrap()["port"], 1);
        let modified = fs::metadata(target.join("background.js")).unwrap().modified().unwrap();
        assert_eq!(prepare(&source, &data, false, 9614).unwrap(), target);
        assert_eq!(fs::metadata(target.join("background.js")).unwrap().modified().unwrap(), modified);
        assert_eq!(fs::read(source.join("background.js")).unwrap(), b"original script");
        assert_eq!(fs::read(source.join(".private")).unwrap(), b"not installed");
        fs::write(source.join("background.js"), b"updated script").unwrap();
        fs::write(source.join("manifest.json"), br#"{"manifest_version":3,"version":"1.0.7"}"#).unwrap();
        assert_eq!(prepare(&source, &data, false, 9614).unwrap(), target);
        assert_eq!(fs::read(target.join("background.js")).unwrap(), b"updated script");
        assert_eq!(read_marker(&target, false).unwrap().version, "1.0.7");
        assert_eq!(prepare(&source, &data, false, 19614).unwrap(), target);
        assert_eq!(small_json(&target.join("app-connection.json")).unwrap()["port"], 19614);
        assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn invalid_source_preserves_the_prepared_extension() {
        let (_root, source, data) = fixture();
        let target = prepare(&source, &data, false, 9614).unwrap();
        let marker = fs::read(target.join(MARKER)).unwrap();
        fs::write(source.join("manifest.json"), b"broken JSON").unwrap();
        assert!(prepare(&source, &data, false, 9614).is_err());
        assert_eq!(fs::read(target.join("background.js")).unwrap(), b"original script");
        assert_eq!(fs::read(target.join(MARKER)).unwrap(), marker);
    }

    #[test]
    fn unknown_folders_and_wrong_flavor_are_never_replaced() {
        let (_root, source, data) = fixture();
        let target = destination(&data, false);
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("personal.txt"), b"keep me").unwrap();
        assert!(prepare(&source, &data, false, 9614).is_err());
        assert_eq!(prepared_dir(&data, false), None);
        assert_eq!(fs::read(target.join("personal.txt")).unwrap(), b"keep me");
        let marker = Marker { owner: "unrelated app".into(), schema: 1, flavor: "chromium".into(), version: "1.0.6".into() };
        fs::write(target.join(MARKER), serde_json::to_vec(&marker).unwrap()).unwrap();
        assert!(prepare(&source, &data, false, 9614).is_err());
        assert_eq!(prepared_dir(&data, false), None);
        assert!(prepare(&source, &data, true, 9614).is_err());
    }

    #[test]
    fn firefox_copy_has_its_own_stable_destination_and_flavor() {
        let (_root, source, data) = fixture();
        fs::write(source.join("manifest.json"), br#"{"manifest_version":3,"version":"1.0.7","browser_specific_settings":{"gecko":{"id":"synthetic@example.invalid"}}}"#).unwrap();
        let target = prepare(&source, &data, true, 9614).unwrap();
        assert_eq!(target, destination(&data, true));
        assert_eq!(prepared_dir(&data, true), Some(target.clone()));
        assert_eq!(prepared_dir(&data, false), None);
        assert_eq!(read_marker(&target, true).unwrap().flavor, "firefox");
        assert!(prepare(&source, &data, false, 9614).is_err());
    }

    #[test]
    fn failed_directory_replacement_rolls_back_the_previous_copy() {
        let (_root, source, data) = fixture();
        let target = prepare(&source, &data, false, 9614).unwrap();
        let parent = target.parent().unwrap();
        assert!(replace(&parent.join("missing-stage"), &target, parent, true).is_err());
        assert_eq!(fs::read(target.join("background.js")).unwrap(), b"original script");
        assert_eq!(prepared_dir(&data, false), Some(target.clone()));
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_rejected_without_following_them() {
        use std::os::unix::fs::symlink;
        let (root, source, data) = fixture();
        let target = prepare(&source, &data, false, 9614).unwrap();
        symlink(root.0.join("outside"), source.join("linked.js")).unwrap();
        assert!(prepare(&source, &data, false, 9614).is_err());
        assert_eq!(fs::read(target.join("background.js")).unwrap(), b"original script");
        fs::remove_file(source.join("linked.js")).unwrap();
        let alternative = root.0.join("alternative");
        fs::create_dir(&alternative).unwrap();
        fs::remove_dir_all(&target).unwrap();
        symlink(&alternative, &target).unwrap();
        assert!(prepare(&source, &data, false, 9614).is_err());
        assert!(fs::read_dir(alternative).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn managed_directories_and_files_have_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (_root, source, data) = fixture();
        let target = prepare(&source, &data, false, 9614).unwrap();
        for path in [&data, target.parent().unwrap(), &target, &target.join("icons")] {
            assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o700);
        }
        for name in [MARKER, "manifest.json", "background.js", "icons/icon.png", "app-connection.json"] {
            assert_eq!(fs::metadata(target.join(name)).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
