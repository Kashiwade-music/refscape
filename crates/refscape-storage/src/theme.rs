//! Portable semantic color themes; their v1 schema is storage-owned.
use crate::{
    document::{read_versioned_json, write_json},
    session::v1,
};
use refscape_model::Theme;
use serde::{Deserialize, Serialize};
use std::path::Path;
const THEME_VERSION: u32 = 1;
#[derive(Serialize, Deserialize)]
struct ThemeDocument {
    version: u32,
    theme: v1::Theme,
}
pub fn save_theme(path: &Path, theme: &Theme) -> Result<(), String> {
    theme.validate()?;
    write_json(
        path,
        &ThemeDocument {
            version: THEME_VERSION,
            theme: theme.into(),
        },
    )
    .map_err(|error| error.to_string())
}
pub fn load_theme(path: &Path) -> Result<Theme, String> {
    let document: ThemeDocument =
        read_versioned_json(path, THEME_VERSION, "theme").map_err(|error| error.to_string())?;
    let theme: Theme = document.theme.into();
    theme.validate()?;
    Ok(theme)
}
