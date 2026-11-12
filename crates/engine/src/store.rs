//! Saving the download list so everything survives a restart or a crash.

use crate::model::DownloadRecord;
use serde::Serialize;
use std::path::Path;

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load_downloads(path: &Path) -> Vec<DownloadRecord> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str(&text) {
        Ok(list) => list,
        Err(_) => {
            // a broken list (the computer went off while it was written...):
            // kept aside, never overwritten by the empty one
            let _ = std::fs::copy(path, path.with_extension("broken.json"));
            Vec::new()
        }
    }
}

/// The download list: saved often, so without spaces (smaller, faster).
pub fn save_downloads(path: &Path, list: &[DownloadRecord]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&list)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
