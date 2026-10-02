//! One-read versioned decoding and streamed atomic document replacement.
use refscape_model::{ErrorKind, RefscapeError};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn read_versioned_json<T: DeserializeOwned>(
    path: &Path,
    expected_version: u32,
    kind: &str,
) -> Result<T, RefscapeError> {
    let bytes = fs::read(path).map_err(|error| {
        RefscapeError::new(
            ErrorKind::Io,
            format!("cannot read {}: {error}", path.display()),
        )
        .with_path(path)
    })?;
    decode_versioned_json(&bytes, expected_version, kind).map_err(|error| error.with_path(path))
}

pub(crate) fn decode_versioned_json<T: DeserializeOwned>(
    bytes: &[u8],
    expected_version: u32,
    kind: &str,
) -> Result<T, RefscapeError> {
    #[derive(Deserialize)]
    struct Envelope {
        version: u64,
    }
    // Unknown fields are skipped without allocating a second JSON value tree.
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|error| {
        RefscapeError::new(
            ErrorKind::InvalidData,
            format!("invalid {kind}: version must be a positive integer ({error})"),
        )
    })?;
    validate_version(envelope.version, expected_version, kind)?;
    serde_json::from_slice(bytes).map_err(|error| {
        RefscapeError::new(ErrorKind::InvalidData, format!("invalid {kind}: {error}"))
    })
}

pub(crate) fn validate_version(
    version: u64,
    expected: u32,
    kind: &str,
) -> Result<(), RefscapeError> {
    if version != u64::from(expected) {
        return Err(RefscapeError::new(
            ErrorKind::Unsupported,
            format!("unsupported {kind} version {version}; supported version is {expected}"),
        ));
    }
    Ok(())
}

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), RefscapeError> {
    stage_and_replace(
        path,
        |writer| {
            serde_json::to_writer_pretty(&mut *writer, value).map_err(std::io::Error::other)?;
            writer.write_all(b"\n")
        },
        |_| Ok(()),
    )
    .map_err(|error| {
        let operation = match error {
            WriteFailure::BeforeReplace(_) => "write-before-replace",
            WriteFailure::ReplacedDurabilityUnconfirmed(_) => "replaced-durability-unconfirmed",
        };
        RefscapeError::new(
            ErrorKind::Io,
            format!("cannot save {}: {error}", path.display()),
        )
        .with_path(path)
        .with_operation(operation)
    })
}

struct PendingFile(PathBuf);
impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Write,
    Flush,
    Sync,
    Replace,
    DirectorySync,
}

#[derive(Debug)]
enum WriteFailure {
    BeforeReplace(std::io::Error),
    ReplacedDurabilityUnconfirmed(std::io::Error),
}
impl From<std::io::Error> for WriteFailure {
    fn from(error: std::io::Error) -> Self {
        Self::BeforeReplace(error)
    }
}
impl std::fmt::Display for WriteFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BeforeReplace(error) => error.fmt(formatter),
            Self::ReplacedDurabilityUnconfirmed(error) => {
                write!(formatter, "replaced; durability unconfirmed: {error}")
            }
        }
    }
}
impl std::error::Error for WriteFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BeforeReplace(error) | Self::ReplacedDurabilityUnconfirmed(error) => Some(error),
        }
    }
}

fn stage_and_replace(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> std::io::Result<()>,
    mut checkpoint: impl FnMut(Stage) -> std::io::Result<()>,
) -> Result<(), WriteFailure> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing file name")
    })?;
    let (pending, file) = loop {
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
            Err(error) => return Err(error.into()),
        }
    };
    let mut writer = BufWriter::new(file);
    checkpoint(Stage::Write)?;
    write(&mut writer)?;
    checkpoint(Stage::Flush)?;
    writer.flush()?;
    checkpoint(Stage::Sync)?;
    writer.get_ref().sync_all()?;
    drop(writer);
    checkpoint(Stage::Replace)?;
    fs::rename(&pending.0, path)?;
    checkpoint(Stage::DirectorySync).map_err(WriteFailure::ReplacedDurabilityUnconfirmed)?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(WriteFailure::ReplacedDurabilityUnconfirmed)?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    stage_and_replace(path, |writer| writer.write_all(bytes), |_| Ok(()))
        .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn faults_before_replace_protect_previous_file_and_cleanup_only_owned_temp() {
        let root = std::env::temp_dir().join(format!(
            "refscape-fault-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("session.json");
        let unrelated = root.join("session.json.other.tmp");
        fs::write(&path, b"old").unwrap();
        fs::write(&unrelated, b"unrelated").unwrap();
        for failure in [Stage::Write, Stage::Flush, Stage::Sync, Stage::Replace] {
            let mut stages = Vec::new();
            let result = stage_and_replace(
                &path,
                |writer| writer.write_all(b"new"),
                |stage| {
                    stages.push(stage);
                    if stage == failure {
                        Err(std::io::Error::other("injected"))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(result.is_err());
            assert_eq!(stages.last(), Some(&failure));
            assert_eq!(fs::read(&path).unwrap(), b"old");
            assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
            assert_eq!(fs::read(&unrelated).unwrap(), b"unrelated");
        }
        let error = stage_and_replace(
            &path,
            |writer| writer.write_all(b"new"),
            |stage| {
                if stage == Stage::DirectorySync {
                    Err(std::io::Error::other("injected"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            WriteFailure::ReplacedDurabilityUnconfirmed(_)
        ));
        assert_eq!(fs::read(&path).unwrap(), b"new");
        fs::remove_file(&path).unwrap();
        fs::remove_file(&unrelated).unwrap();
        fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn serialization_failure_preserves_previous_document() {
        struct Invalid;
        impl Serialize for Invalid {
            fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("injected serializer failure"))
            }
        }
        let path =
            std::env::temp_dir().join(format!("refscape-serialize-{}.json", std::process::id()));
        fs::write(&path, b"old").unwrap();
        assert!(write_json(&path, &Invalid).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        fs::remove_file(path).unwrap();
    }
}
