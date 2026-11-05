//! The app's data folder and crash-safe file writes.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// `%LOCALAPPDATA%\aion2dps`, or the temp directory when that variable is missing.
pub fn folder() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("aion2dps")
}

/// Write next to `path` and rename over it, so a crash mid-write leaves
/// either the old file or the new one, never a partial one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = temp_path(path);
    let written = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    let result = written.and_then(|()| std::fs::rename(&temp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

pub fn temp_path(path: &Path) -> PathBuf {
    with_suffix(path, ".tmp")
}

pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}
