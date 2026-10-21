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
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_downloads(path: &Path, list: &[DownloadRecord]) -> anyhow::Result<()> {
    write_json_atomic(path, &list)
}
