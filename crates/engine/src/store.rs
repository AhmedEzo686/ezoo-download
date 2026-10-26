//! Saving the download list so everything survives a restart or a crash.

use crate::model::DownloadRecord;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

struct JsonTemp {
    path: std::path::PathBuf,
    file: Option<std::fs::File>,
}

impl Drop for JsonTemp {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    // Serialize first: a protection/serialization failure must leave the
    // previous settings file intact and never write a plaintext fallback.
    let contents = serde_json::to_vec_pretty(value)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    for _ in 0..5 {
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary = path.with_extension(format!("json.tmp-{}-{nanos:x}-{n}", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = match options.open(&temporary) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        };
        let mut tmp = JsonTemp { path: temporary, file: Some(file) };
        tmp.file.as_mut().expect("new JSON file").write_all(&contents)?;
        drop(tmp.file.take());
        std::fs::rename(&tmp.path, path)?;
        return Ok(());
    }
    anyhow::bail!("could not create a private settings file")
}

pub fn load_downloads(path: &Path) -> Vec<DownloadRecord> {
    try_load_downloads(path).unwrap_or_default()
}

/// Startup must preserve an unreadable/encrypted history file, rather than
/// treating a temporarily unavailable credential key as an empty history.
pub fn try_load_downloads(path: &Path) -> anyhow::Result<Vec<DownloadRecord>> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(serde_json::from_str(&contents)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

pub fn save_downloads(path: &Path, list: &[DownloadRecord]) -> anyhow::Result<()> {
    write_json_atomic(path, &list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("ezoo-store-{label}-{}-{nanos:x}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn failed_serialization_preserves_the_previous_file() {
        struct Refuse;
        impl Serialize for Refuse {
            fn serialize<S: serde::Serializer>(&self, _s: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("synthetic protection failure"))
            }
        }
        let dir = test_dir("failure");
        let path = dir.join("settings.json");
        write_json_atomic(&path, &serde_json::json!({"previous": true})).unwrap();
        let previous = std::fs::read(&path).unwrap();
        assert!(write_json_atomic(&path, &Refuse).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn saves_private_files_without_changing_parent_permissions_or_following_old_temp_symlinks() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = test_dir("private");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = dir.join("settings.json");
        let other = dir.join("other");
        std::fs::write(&other, "untouched").unwrap();
        symlink(&other, path.with_extension("json.tmp")).unwrap();
        write_json_atomic(&path, &serde_json::json!({"saved": true})).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(std::fs::read_to_string(other).unwrap(), "untouched");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
