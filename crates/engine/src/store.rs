//! Saving the download list so everything survives a restart or a crash.

use crate::model::DownloadRecord;
use serde::Serialize;
use std::path::Path;

/// Written to a new file first, made to reach the disk (not only Windows'
/// memory: a power cut right after a save left an empty or zero-filled list,
/// and every download was gone), then put in place of the old one.
fn write_durable(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// The last good copy of a list (`name.bak.json`), renewed at most once a
/// minute: what is read when the list itself is broken.
fn backup_of(path: &Path) -> std::path::PathBuf {
    path.with_extension("bak.json")
}

fn keep_backup(path: &Path) {
    let bak = backup_of(path);
    let fresh = std::fs::metadata(&bak)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < std::time::Duration::from_secs(60));
    if fresh {
        return;
    }
    if let Ok(data) = std::fs::read(path) {
        // only a list that reads back is kept as the good copy
        if serde_json::from_slice::<serde_json::Value>(&data).is_ok() {
            let _ = write_durable(&bak, &data);
        }
    }
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    write_durable(path, &serde_json::to_vec_pretty(value)?)?;
    keep_backup(path);
    Ok(())
}

/// A JSON file as it was saved last: the file itself, else its last good
/// copy, else a save that was nearly done (the computer went off between
/// writing and putting it in place). A broken file is kept aside.
pub fn read_json_kept<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let mut broken = false;
    for p in [path.to_path_buf(), backup_of(path), path.with_extension("json.tmp")] {
        let Ok(text) = std::fs::read(&p) else { continue };
        match serde_json::from_slice::<T>(&text) {
            Ok(v) => {
                if broken {
                    eprintln!("{} was broken: read from {}", path.display(), p.display());
                }
                return Some(v);
            }
            Err(_) => {
                if p == path {
                    broken = true;
                    // never overwritten by the next save: kept to look at
                    let _ = std::fs::copy(path, path.with_extension("broken.json"));
                }
            }
        }
    }
    None
}

pub fn load_downloads(path: &Path) -> Vec<DownloadRecord> {
    read_json_kept(path).unwrap_or_default()
}

/// The download list: saved often, so without spaces (smaller, faster).
pub fn save_downloads(path: &Path, list: &[DownloadRecord]) -> anyhow::Result<()> {
    write_durable(path, &serde_json::to_vec(&list)?)?;
    keep_backup(path);
    Ok(())
}
