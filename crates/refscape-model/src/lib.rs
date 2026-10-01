//! UI-independent source, canvas, session, and theme models and invariants.

use std::{collections::HashSet, path::PathBuf};

use serde::{Deserialize, Serialize};

pub const SESSION_VERSION: u32 = 1;
pub const MIN_ZOOM: f32 = 0.15;
pub const MAX_ZOOM: f32 = 3.0;

/// Language backend used to analyze a source project.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectLanguage {
    #[default]
    Auto,
    Rust,
    Cpp,
}

/// Analysis settings are independent of the source root and travel with sessions.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectOptions {
    #[serde(default)]
    pub language: ProjectLanguage,
    #[serde(default)]
    pub compilation_database: Option<PathBuf>,
}

/// Zero-based source coordinates. `character` counts UTF-16 code units (LSP).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// Half-open source range, in document coordinates.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRange {
    pub start: Position,
    pub end: Position,
}

impl SourceRange {
    pub fn contains(self, position: Position) -> bool {
        self.start <= position && position < self.end
    }

    pub fn validate(self) -> Result<(), String> {
        if self.start > self.end {
            return Err("Source range starts after its end".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: PathBuf,
    pub range: SourceRange,
    pub selection_range: SourceRange,
    #[serde(default)]
    pub children: Vec<Symbol>,
}

impl Symbol {
    pub fn file(path: PathBuf, range: SourceRange) -> Self {
        Self {
            id: format!("{}:file", path.to_string_lossy()),
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            kind: "file".into(),
            path,
            range,
            selection_range: SourceRange {
                start: range.start,
                end: range.start,
            },
            children: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.range.validate()?;
        self.selection_range.validate()?;
        if self.id.is_empty() || self.path.as_os_str().is_empty() {
            return Err("Symbol ID and path must be nonempty".into());
        }
        if self.selection_range.start < self.range.start
            || self.selection_range.end > self.range.end
        {
            return Err("Symbol selection must lie inside its source range".into());
        }
        for child in &self.children {
            child.validate()?;
        }
        Ok(())
    }
}

/// Semantic tokens retain absolute document coordinates, even on excerpt cards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticToken {
    pub line: u32,
    pub start: u32,
    pub length: u32,
    pub kind: String,
    #[serde(default)]
    pub modifiers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDocument {
    pub symbol: Symbol,
    pub code: String,
    #[serde(default)]
    pub tokens: Vec<SemanticToken>,
}

impl SourceDocument {
    /// Classification comes from semantic tokens supplied by the language backend.
    pub fn variable_token(&self, position: Position) -> Option<&SemanticToken> {
        self.tokens.iter().find(|token| {
            token.line == position.line
                && token.start <= position.character
                && token
                    .start
                    .checked_add(token.length)
                    .is_some_and(|end| position.character < end)
                && matches!(token.kind.as_str(), "variable" | "parameter" | "property")
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        self.symbol.validate()?;
        if self
            .tokens
            .iter()
            .any(|token| token.length == 0 || token.start.checked_add(token.length).is_none())
        {
            return Err("Semantic tokens must have a valid nonempty span".into());
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub offset: Point,
    pub zoom: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            offset: Point::default(),
            zoom: 1.0,
        }
    }
}

impl Viewport {
    pub fn world_to_screen(self, point: Point) -> Point {
        Point::new(
            point.x * self.zoom + self.offset.x,
            point.y * self.zoom + self.offset.y,
        )
    }

    pub fn screen_to_world(self, point: Point) -> Point {
        Point::new(
            (point.x - self.offset.x) / self.zoom,
            (point.y - self.offset.y) / self.zoom,
        )
    }

    pub fn validate(self) -> Result<(), String> {
        if !self.offset.is_finite()
            || !self.zoom.is_finite()
            || !(MIN_ZOOM..=MAX_ZOOM).contains(&self.zoom)
        {
            return Err("Viewport must have finite coordinates and zoom between 0.15 and 3".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeCard {
    pub id: String,
    pub source: SourceDocument,
    pub position: Point,
    pub width: f32,
    pub height: f32,
}

pub const CODE_CARD_HEADER: f32 = 52.0;
pub const CODE_LINE_HEIGHT: f32 = 20.0;
pub const CODE_REGION_PADDING: f32 = 22.0;
pub const CODE_REGION_HEADER: f32 = 36.0;

impl CodeCard {
    /// World-space height shared by painting and collision detection, including
    /// source that outgrew the dimensions stored in an older session.
    pub fn display_height(&self) -> f32 {
        self.height.max(Self::source_height(&self.source))
    }

    pub fn source_height(source: &SourceDocument) -> f32 {
        (CODE_CARD_HEADER + source.code.lines().count().max(1) as f32 * CODE_LINE_HEIGHT + 24.0)
            .max(128.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    Definition,
    TypeDefinition,
    Reference,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: ConnectionKind,
    pub source: Position,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
    pub card_ids: Vec<String>,
}

/// Package container reported by the language backend's official project metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCrate {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    pub background: String,
    pub surface: String,
    pub surface_alt: String,
    pub text: String,
    pub muted: String,
    pub accent: String,
    pub border: String,
    pub connection: String,
    pub syntax_keyword: String,
    pub syntax_string: String,
    pub syntax_type: String,
    pub syntax_function: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub palette: Palette,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self::from_colors(
            "Dark",
            [
                "#111820", "#1A2430", "#243342", "#E5EDF5", "#94A9BC", "#60D7BD", "#344858",
                "#62B8C5", "#CB9BF4", "#A4CD85", "#EAC071", "#79C5E9",
            ],
        )
    }

    pub fn light() -> Self {
        Self::from_colors(
            "Light",
            [
                "#EDF2F5", "#FFFFFF", "#F2F6F8", "#233443", "#657787", "#087F73", "#CAD6DD",
                "#438C9F", "#8554AB", "#397A31", "#99682B", "#246D9B",
            ],
        )
    }

    fn from_colors(name: &str, colors: [&str; 12]) -> Self {
        let [
            background,
            surface,
            surface_alt,
            text,
            muted,
            accent,
            border,
            connection,
            syntax_keyword,
            syntax_string,
            syntax_type,
            syntax_function,
        ] = colors.map(str::to_owned);
        Self {
            name: name.into(),
            palette: Palette {
                background,
                surface,
                surface_alt,
                text,
                muted,
                accent,
                border,
                connection,
                syntax_keyword,
                syntax_string,
                syntax_type,
                syntax_function,
            },
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let p = &self.palette;
        for color in [
            &p.background,
            &p.surface,
            &p.surface_alt,
            &p.text,
            &p.muted,
            &p.accent,
            &p.border,
            &p.connection,
            &p.syntax_keyword,
            &p.syntax_string,
            &p.syntax_type,
            &p.syntax_function,
        ] {
            if color.len() != 7
                || !color.starts_with('#')
                || !color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
            {
                return Err(format!("Invalid theme color {color}; expected #RRGGBB"));
            }
        }
        if self.name.trim().is_empty() {
            return Err("Theme name must be nonempty".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub project_root: PathBuf,
    #[serde(default)]
    pub project_options: ProjectOptions,
    pub cards: Vec<CodeCard>,
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub regions: Vec<Region>,
    pub viewport: Viewport,
    pub theme: Theme,
}

impl Session {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            version: SESSION_VERSION,
            project_root,
            project_options: ProjectOptions::default(),
            cards: Vec::new(),
            connections: Vec::new(),
            regions: Vec::new(),
            viewport: Viewport::default(),
            theme: Theme::default(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != SESSION_VERSION {
            return Err(format!("Unsupported session version {}", self.version));
        }
        if self.project_root.as_os_str().is_empty() {
            return Err("Session project root is empty".into());
        }
        self.viewport.validate()?;
        self.theme.validate()?;
        let mut cards = HashSet::new();
        for card in &self.cards {
            if card.id.is_empty() || !cards.insert(card.id.as_str()) {
                return Err("Card IDs must be nonempty and unique".into());
            }
            if !card.position.is_finite()
                || !card.width.is_finite()
                || !card.height.is_finite()
                || card.width <= 0.0
                || card.height <= 0.0
            {
                return Err("Card geometry must be finite and positive".into());
            }
            card.source.validate()?;
        }
        let mut connections = HashSet::new();
        for connection in &self.connections {
            if connection.id.is_empty() || !connections.insert(&connection.id) {
                return Err("Connection IDs must be nonempty and unique".into());
            }
            if !cards.contains(connection.from.as_str()) || !cards.contains(connection.to.as_str())
            {
                return Err("Connection refers to a missing card".into());
            }
        }
        let mut regions = HashSet::new();
        for region in &self.regions {
            if region.id.is_empty() || !regions.insert(&region.id) {
                return Err("Region IDs must be nonempty and unique".into());
            }
            if region
                .card_ids
                .iter()
                .any(|id| !cards.contains(id.as_str()))
            {
                return Err("Region refers to a missing card".into());
            }
        }
        Ok(())
    }
}

/// Translate a UTF-16 column to a UTF-8 byte boundary, rejecting split surrogates.
pub fn utf16_byte_offset(text: &str, column: u32) -> Option<usize> {
    let mut units = 0_u32;
    for (offset, ch) in text.char_indices() {
        if units == column {
            return Some(offset);
        }
        units += ch.len_utf16() as u32;
        if units > column {
            return None;
        }
    }
    (units == column).then_some(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets_preserve_unicode_boundaries() {
        assert_eq!(utf16_byte_offset("a😀猫", 1), Some(1));
        assert_eq!(utf16_byte_offset("a😀猫", 2), None);
        assert_eq!(utf16_byte_offset("a😀猫", 3), Some(5));
        assert_eq!(utf16_byte_offset("a😀猫", 4), Some(8));
        assert_eq!(utf16_byte_offset("a😀猫", 5), None);
    }

    #[test]
    fn invalid_viewports_and_themes_are_rejected() {
        assert!(
            Viewport {
                zoom: f32::NAN,
                ..Viewport::default()
            }
            .validate()
            .is_err()
        );
        let mut theme = Theme::light();
        theme.palette.accent = "blue".into();
        assert!(theme.validate().is_err());
        assert!(Theme::dark().validate().is_ok());
    }

    #[test]
    fn session_rejects_dangling_edges_and_future_versions() {
        let mut session = Session::new("/project".into());
        session.connections.push(Connection {
            id: "edge".into(),
            from: "missing".into(),
            to: "missing".into(),
            kind: ConnectionKind::Definition,
            source: Position::default(),
        });
        assert!(session.validate().is_err());
        session.connections.clear();
        session.version += 1;
        assert!(session.validate().is_err());
    }
}
