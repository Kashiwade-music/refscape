//! Portable semantic color themes.

use std::path::Path;

use refscape_model::Theme;
use serde::{Deserialize, Serialize};

use crate::document::{read_versioned_json, write_json};

const THEME_VERSION: u32 = 1;

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
