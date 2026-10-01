//! Shared JSON validation and atomic document replacement.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Serialize, de::DeserializeOwned};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn read_versioned_json(
    path: &Path,
    expected_version: u32,
    kind: &str,
) -> Result<serde_json::Value, String> {
    let document: serde_json::Value = read_json(path)?;
    let version = document
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("invalid {kind}: version must be a positive integer"))?;
    validate_version(version, expected_version, kind)?;
    Ok(document)
}

pub(crate) fn validate_version(version: u64, expected: u32, kind: &str) -> Result<(), String> {
    if version != u64::from(expected) {
        return Err(format!(
            "unsupported {kind} version {version}; supported version is {expected}"
        ));
    }
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let file =
        File::open(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_reader(file)
        .map_err(|error| format!("invalid JSON {}: {error}", path.display()))
}

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    // Serialization happens before touching the previous document.
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("cannot serialize {}: {error}", path.display()))?;
    bytes.push(b'\n');
    atomic_write(path, &bytes).map_err(|error| format!("cannot save {}: {error}", path.display()))
}

struct PendingFile(PathBuf);

impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Stage in the same directory so replacement cannot cross filesystem boundaries.
/// The previous file is never deleted first. An interrupted staging write leaves
/// the previous document intact, and readers ignore uncommitted temporary files.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing file name")
    })?;
    let (pending, mut file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut temporary_name = name.to_os_string();
        temporary_name.push(format!(".{}.{}.tmp", std::process::id(), sequence));
        let temporary = parent.join(temporary_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (PendingFile(temporary), file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&pending.0, path)?;
    // POSIX needs the directory entry synced as well as the staged file's contents.
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}
