//! User settings and platform configuration paths.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::document::{read_versioned_json, validate_version, write_json};

const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub last_project: Option<PathBuf>,
    pub theme_file: Option<PathBuf>,
    /// Overrides the rust-analyzer executable discovered on PATH.
    pub rust_analyzer_path: Option<PathBuf>,
    /// Overrides the clangd executable discovered on PATH.
    #[serde(default)]
    pub clangd_path: Option<PathBuf>,
    /// Overrides the TypeScript language server executable or Node entry point.
    #[serde(default)]
    pub typescript_language_server_path: Option<PathBuf>,
    /// Overrides the Pyright language server executable or Node entry point.
    #[serde(default)]
    pub pyright_path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            last_project: None,
            theme_file: None,
            rust_analyzer_path: None,
            clangd_path: None,
            typescript_language_server_path: None,
            pyright_path: None,
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
