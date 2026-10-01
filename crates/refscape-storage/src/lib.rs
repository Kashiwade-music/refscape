//! Versioned JSON persistence with atomic replacement of saved documents.

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use refscape_application::SessionRepository;
use refscape_model::{SESSION_VERSION, Session, Theme};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

const SETTINGS_VERSION: u32 = 1;
const THEME_VERSION: u32 = 1;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Stateless repository: callers choose paths, so named sessions can be shared.
#[derive(Debug, Default, Clone, Copy)]
pub struct JsonSessionRepository;

impl SessionRepository for JsonSessionRepository {
    fn save(&self, path: &Path, session: &Session) -> Result<(), String> {
        session.validate()?;
        write_json(path, session)
    }

    fn load(&self, path: &Path) -> Result<Session, String> {
        // Check the format version before deserializing a potentially newer schema.
        let document = read_versioned_json(path, SESSION_VERSION, "session")?;
        let session: Session = serde_json::from_value(document)
            .map_err(|error| format!("invalid session {}: {error}", path.display()))?;
        session.validate()?;
        Ok(session)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub last_project: Option<PathBuf>,
    pub theme_file: Option<PathBuf>,
    /// Overrides the rust-analyzer executable discovered on PATH.
    pub rust_analyzer_path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            last_project: None,
            theme_file: None,
            rust_analyzer_path: None,
        }
    }
}

/// A missing settings file is a normal first launch. Invalid files are errors.
pub fn load_settings(path: &Path) -> Result<Settings, String> {
    match fs::metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default());
        }
        Err(error) => {
            return Err(format!("cannot read {}: {error}", path.display()));
        }
    }
    let document = read_versioned_json(path, SETTINGS_VERSION, "settings")?;
    serde_json::from_value(document)
        .map_err(|error| format!("invalid settings {}: {error}", path.display()))
}

pub fn save_settings(path: &Path, settings: &Settings) -> Result<(), String> {
    validate_version(u64::from(settings.version), SETTINGS_VERSION, "settings")?;
    write_json(path, settings)
}

#[derive(Serialize, Deserialize)]
struct ThemeDocument {
    version: u32,
    theme: Theme,
}

/// Custom themes are portable, versioned JSON files containing semantic colors.
pub fn save_theme(path: &Path, theme: &Theme) -> Result<(), String> {
    theme.validate()?;
    write_json(
        path,
        &ThemeDocument {
            version: THEME_VERSION,
            theme: theme.clone(),
        },
    )
}

pub fn load_theme(path: &Path) -> Result<Theme, String> {
    let document = read_versioned_json(path, THEME_VERSION, "theme")?;
    let document: ThemeDocument = serde_json::from_value(document)
        .map_err(|error| format!("invalid theme {}: {error}", path.display()))?;
    document.theme.validate()?;
    Ok(document.theme)
}

pub fn default_session_path(project_root: &Path) -> PathBuf {
    project_root.join(".refscape").join("session.json")
}

/// Resolve the platform's per-user configuration directory without creating it.
pub fn default_config_directory() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        let root = env::var_os("APPDATA").ok_or("APPDATA is not set")?;
        Ok(PathBuf::from(root).join("Refscape"))
    }
    #[cfg(target_os = "macos")]
    {
        let root = env::var_os("HOME").ok_or("HOME is not set")?;
        Ok(PathBuf::from(root).join("Library/Application Support/Refscape"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Some(root) = env::var_os("XDG_CONFIG_HOME")
            && Path::new(&root).is_absolute()
        {
            return Ok(PathBuf::from(root).join("refscape"));
        }
        let root = env::var_os("HOME").ok_or("HOME is not set")?;
        Ok(PathBuf::from(root).join(".config/refscape"))
    }
}

fn read_versioned_json(
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

fn validate_version(version: u64, expected: u32, kind: &str) -> Result<(), String> {
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

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
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
fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use refscape_model::{
        CodeCard, Connection, ConnectionKind, Point, Position, Region, SourceDocument, SourceRange,
        Symbol, Viewport,
    };

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let id = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path =
                env::temp_dir().join(format!("refscape-storage-test-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self, file: &str) -> PathBuf {
            self.0.join(file)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn settings_roundtrip_and_first_launch_defaults() {
        let directory = TestDirectory::new();
        let path = directory.path("config/settings.json");
        assert_eq!(load_settings(&path).unwrap(), Settings::default());
        let settings = Settings {
            last_project: Some(PathBuf::from("workspace/日本語")),
            theme_file: Some(PathBuf::from("themes/custom.json")),
            rust_analyzer_path: Some(PathBuf::from("tools/rust-analyzer")),
            ..Settings::default()
        };
        save_settings(&path, &settings).unwrap();
        assert_eq!(load_settings(&path).unwrap(), settings);
        save_settings(&path, &Settings::default()).unwrap();
        assert_eq!(load_settings(&path).unwrap(), Settings::default());
    }

    #[test]
    fn theme_roundtrip_preserves_custom_colors() {
        let directory = TestDirectory::new();
        let path = directory.path("theme.json");
        let mut theme = Theme::dark();
        theme.name = "Custom theme".into();
        theme.palette.accent = "#bb55ff".into();
        save_theme(&path, &theme).unwrap();
        assert_eq!(load_theme(&path).unwrap(), theme);
    }

    #[test]
    fn session_roundtrip_preserves_canvas_code_connections_and_theme() {
        let directory = TestDirectory::new();
        let path = directory.path("nested/exploration.json");
        let mut session = Session::new(directory.0.clone());
        let range = SourceRange {
            start: Position::new(0, 0),
            end: Position::new(1, 0),
        };
        for (id, x) in [("main", -30.5), ("run", 700.25)] {
            session.cards.push(CodeCard {
                id: id.into(),
                source: SourceDocument {
                    symbol: Symbol::file(directory.path(&format!("{id}.rs")), range),
                    code: format!("fn {id}() {{}}\n"),
                    tokens: vec![],
                },
                position: Point::new(x, 125.0),
                width: 600.0,
                height: 320.0,
            });
        }
        session.connections.push(Connection {
            id: "main-to-run".into(),
            from: "main".into(),
            to: "run".into(),
            kind: ConnectionKind::Definition,
            source: Position::new(0, 3),
        });
        session.connections.push(Connection {
            id: "main-to-type".into(),
            from: "main".into(),
            to: "run".into(),
            kind: ConnectionKind::TypeDefinition,
            source: Position::new(0, 3),
        });
        session.regions.push(Region {
            id: "crate".into(),
            label: "Example crate".into(),
            path: directory.0.clone(),
            card_ids: vec!["main".into(), "run".into()],
        });
        session.viewport = Viewport {
            offset: Point::new(-220.0, 100.0),
            zoom: 0.75,
        };
        session.theme = Theme::light();
        JsonSessionRepository.save(&path, &session).unwrap();
        assert_eq!(JsonSessionRepository.load(&path).unwrap(), session);
        session.cards[0].position = Point::new(999.0, -222.0);
        JsonSessionRepository.save(&path, &session).unwrap();
        assert_eq!(JsonSessionRepository.load(&path).unwrap(), session);
    }

    #[test]
    fn invalid_session_does_not_overwrite_last_valid_session() {
        let directory = TestDirectory::new();
        let path = directory.path("session.json");
        let session = Session::new(directory.0.clone());
        JsonSessionRepository.save(&path, &session).unwrap();
        let mut invalid = session.clone();
        invalid.viewport.zoom = f32::NAN;
        assert!(JsonSessionRepository.save(&path, &invalid).is_err());
        assert_eq!(JsonSessionRepository.load(&path).unwrap(), session);
    }

    #[test]
    fn rejects_malformed_and_future_formats() {
        let directory = TestDirectory::new();
        let path = directory.path("session.json");
        fs::write(&path, "{not JSON").unwrap();
        assert!(JsonSessionRepository.load(&path).is_err());
        fs::write(&path, r#"{"version":999,"new_schema":true}"#).unwrap();
        let error = JsonSessionRepository.load(&path).unwrap_err();
        assert!(error.contains("unsupported session version 999"));
        fs::write(&path, r#"{"version":"1"}"#).unwrap();
        assert!(JsonSessionRepository.load(&path).is_err());
        fs::write(&path, r#"{"version":999}"#).unwrap();
        assert!(load_settings(&path).is_err());
        assert!(load_theme(&path).is_err());
    }

    #[test]
    fn invalid_theme_cannot_replace_a_previous_theme() {
        let directory = TestDirectory::new();
        let path = directory.path("theme.json");
        let theme = Theme::light();
        save_theme(&path, &theme).unwrap();
        let mut invalid = theme.clone();
        invalid.palette.accent = "not a color".into();
        assert!(save_theme(&path, &invalid).is_err());
        assert_eq!(load_theme(&path).unwrap(), theme);
    }

    #[test]
    fn interrupted_staging_file_does_not_replace_committed_document() {
        let directory = TestDirectory::new();
        let path = directory.path("settings.json");
        let settings = Settings::default();
        save_settings(&path, &settings).unwrap();
        fs::write(directory.path("settings.json.123.456.tmp"), "{partial").unwrap();
        assert_eq!(load_settings(&path).unwrap(), settings);
        save_settings(&path, &settings).unwrap();
        assert_eq!(load_settings(&path).unwrap(), settings);
    }

    #[test]
    fn failed_commit_cleans_up_staging_file() {
        let directory = TestDirectory::new();
        let path = directory.path("existing-directory");
        fs::create_dir(&path).unwrap();
        assert!(atomic_write(&path, b"payload").is_err());
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }
}
