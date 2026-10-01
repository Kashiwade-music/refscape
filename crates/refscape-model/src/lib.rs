//! UI-independent source, canvas, session, and theme models and invariants.

use std::{borrow::Cow, collections::HashSet, path::PathBuf};

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
    /// Ancestor declaration excerpts supplied by the official language backend.
    #[serde(default)]
    pub context: Vec<SourceContext>,
    /// Includes the first line's indentation without changing the symbol's identity.
    #[serde(default)]
    pub code_start: Option<Position>,
    /// Source snapshots of the gaps between ancestor declarations and the symbol body.
    #[serde(default)]
    pub folded: Vec<SourceContext>,
    /// Revealed gap snapshots retained so each section can be folded again.
    #[serde(default)]
    pub expanded: Vec<SourceContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceContext {
    pub start_line: u32,
    pub code: String,
}

pub struct SourceLine<'a> {
    pub position: Option<Position>,
    pub text: Cow<'a, str>,
    pub fold: Option<usize>,
}

impl SourceDocument {
    /// Reserve separate control and number columns using the entire excerpt's line range.
    /// Revealing hidden rows cannot change the code or line-number column's position.
    pub fn code_gutter_width(&self) -> f32 {
        let start = self.code_start.unwrap_or(self.symbol.range.start).line;
        let last = (u64::from(start) + self.code.lines().count() as u64)
            .max(u64::from(self.symbol.range.end.line) + 1);
        let digits = last.to_string().len().max(3);
        24.0 + digits as f32 * 8.0 + 12.0
    }

    /// Display rows retain document coordinates; folded gaps have no source position.
    pub fn display_lines(&self) -> Vec<SourceLine<'_>> {
        let mut lines = Vec::new();
        let start = self.code_start.unwrap_or(self.symbol.range.start);
        for (index, context) in self.context.iter().enumerate() {
            let mut end_line = context.start_line;
            for (row, text) in context.code.lines().enumerate() {
                end_line = context.start_line + row as u32;
                lines.push(SourceLine {
                    position: Some(Position::new(end_line, 0)),
                    text: Cow::Borrowed(text),
                    fold: self
                        .expanded
                        .iter()
                        .any(|gap| gap.start_line == end_line)
                        .then_some(index),
                });
            }
            let next_line = self
                .context
                .get(index + 1)
                .map_or(start.line, |c| c.start_line);
            if end_line + 1 < next_line {
                lines.push(SourceLine {
                    position: None,
                    text: Cow::Owned(format!("    ... (Show {} Lines)", next_line - end_line - 1)),
                    fold: Some(index),
                });
            }
        }
        lines.extend(self.code.lines().enumerate().map(|(row, text)| SourceLine {
            position: Some(Position::new(
                start.line + row as u32,
                if row == 0 { start.character } else { 0 },
            )),
            text: Cow::Borrowed(text),
            fold: None,
        }));
        lines
    }

    pub fn display_row(&self, position: Position) -> Option<usize> {
        self.display_lines().iter().position(|line| {
            line.position
                .is_some_and(|start| start.line == position.line)
        })
    }

    /// Links from temporarily hidden code stay anchored to its collapsed section.
    pub fn display_anchor_row(&self, position: Position) -> Option<usize> {
        self.display_row(position).or_else(|| {
            self.display_lines().iter().position(|line| {
                line.position.is_none()
                    && line.fold.is_some_and(|index| {
                        self.folded_range(index)
                            .is_some_and(|range| range.contains(&position.line))
                    })
            })
        })
    }

    pub fn expanded_context(&self, index: usize) -> Option<&SourceContext> {
        let context = self.context.get(index)?;
        let end = context
            .start_line
            .checked_add(context.code.lines().count() as u32)?;
        self.expanded.iter().find(|gap| {
            gap.start_line > context.start_line
                && gap.start_line.checked_add(gap.code.lines().count() as u32) == Some(end)
        })
    }

    /// Recover controls for snapshots saved before revealed spans had explicit metadata.
    /// Declaration boundaries come from the backend; all revealed text stays from the snapshot.
    pub fn recover_expanded_context(&mut self, declarations: &[SourceContext]) {
        for (index, context) in self.context.iter().enumerate() {
            if self.expanded_context(index).is_some() || self.folded_range(index).is_some() {
                continue;
            }
            let Some(header) = declarations
                .iter()
                .find(|header| header.start_line == context.start_line)
            else {
                continue;
            };
            let count = header.code.lines().count();
            if count == 0
                || count >= context.code.lines().count()
                || !context.code.lines().take(count).eq(header.code.lines())
            {
                continue;
            }
            self.expanded.push(SourceContext {
                start_line: context.start_line + count as u32,
                code: format!(
                    "{}\n",
                    context
                        .code
                        .lines()
                        .skip(count)
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            });
        }
    }

    /// Only real, currently displayed source can be used for navigation, including context.
    pub fn contains_display_position(&self, position: Position) -> bool {
        if self.symbol.range.contains(position) {
            return true;
        }
        self.context.iter().any(|context| {
            position
                .line
                .checked_sub(context.start_line)
                .and_then(|row| context.code.lines().nth(row as usize))
                .is_some_and(|text| position.character < text.encode_utf16().count() as u32)
        })
    }

    pub fn folded_range(&self, index: usize) -> Option<std::ops::Range<u32>> {
        let context = self.context.get(index)?;
        let start = context
            .start_line
            .checked_add(context.code.lines().count() as u32)?;
        let end = self.context.get(index + 1).map_or(
            self.code_start.unwrap_or(self.symbol.range.start).line,
            |next| next.start_line,
        );
        (start < end).then_some(start..end)
    }

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
        let start = self.code_start.unwrap_or(self.symbol.range.start);
        if start.line != self.symbol.range.start.line
            || start.character > self.symbol.range.start.character
        {
            return Err("Source excerpt must start on the symbol's first line".into());
        }
        let mut previous_end = None;
        for context in &self.context {
            let count = u32::try_from(context.code.lines().count())
                .map_err(|_| "Source context is too long")?;
            let end = context
                .start_line
                .checked_add(count)
                .ok_or("Source context line overflow")?;
            if count == 0
                || end > start.line
                || previous_end.is_some_and(|previous| previous > context.start_line)
            {
                return Err("Source context must precede the excerpt in source order".into());
            }
            previous_end = Some(end);
        }
        let mut seen = HashSet::new();
        for folded in &self.folded {
            if !seen.insert(folded.start_line)
                || !self.context.iter().enumerate().any(|(index, _)| {
                    self.folded_range(index).is_some_and(|range| {
                        range.start == folded.start_line
                            && folded.code.lines().count() == (range.end - range.start) as usize
                    })
                })
            {
                return Err("Folded source must match an omitted context range".into());
            }
        }
        for expanded in &self.expanded {
            if expanded.code.lines().count() == 0
                || !seen.insert(expanded.start_line)
                || !self.context.iter().enumerate().any(|(index, context)| {
                    self.expanded_context(index).is_some_and(|gap| {
                        gap == expanded
                            && context
                                .code
                                .lines()
                                .skip((gap.start_line - context.start_line) as usize)
                                .eq(gap.code.lines())
                    })
                })
            {
                return Err("Expanded source must match the end of its context section".into());
            }
        }
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
        (CODE_CARD_HEADER + source.display_lines().len().max(1) as f32 * CODE_LINE_HEIGHT + 24.0)
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
    fn folded_display_retains_source_coordinates_and_counts_context_in_card_height() {
        let range = SourceRange {
            start: Position::new(74, 4),
            end: Position::new(74, 20),
        };
        let mut source = SourceDocument {
            expanded: Vec::new(),
            folded: Vec::new(),
            symbol: Symbol::file("project.rs".into(), range),
            code: "    fn options() {}".into(),
            code_start: Some(Position::new(74, 0)),
            tokens: vec![],
            context: vec![SourceContext {
                start_line: 51,
                code: "impl CppProject {".into(),
            }],
        };
        source.validate().unwrap();
        let lines = source.display_lines();
        assert_eq!(lines[0].position, Some(Position::new(51, 0)));
        assert_eq!(lines[1].position, None);
        assert_eq!(lines[1].text, "    ... (Show 22 Lines)");
        assert_eq!(lines[2].position, Some(Position::new(74, 0)));
        assert_eq!(source.display_row(Position::new(74, 7)), Some(2));
        assert_eq!(CodeCard::source_height(&source), 136.0);
        source.context[0].start_line = 74;
        assert!(source.validate().is_err());
    }

    #[test]
    fn gutter_reserves_full_source_line_numbers_even_while_context_is_folded() {
        let range = SourceRange {
            start: Position::new(10_000, 4),
            end: Position::new(10_000, 20),
        };
        let mut source = SourceDocument {
            symbol: Symbol::file("project.rs".into(), range),
            code: "    fn options() {}".into(),
            tokens: vec![],
            code_start: None,
            context: vec![SourceContext {
                start_line: 9_998,
                code: "impl Project {".into(),
            }],
            folded: vec![],
            expanded: vec![],
        };
        source.validate().unwrap();
        assert_eq!(source.code_gutter_width(), 76.0);
        assert_eq!(source.display_lines()[1].text, "    ... (Show 1 Lines)");
        source.context[0].code.push_str("\n    fn first() {}\n");
        source.validate().unwrap();
        assert_eq!(source.code_gutter_width(), 76.0);
    }

    #[test]
    fn legacy_revealed_context_recovers_controls_from_backend_declaration_boundaries() {
        let range = SourceRange {
            start: Position::new(5, 4),
            end: Position::new(5, 20),
        };
        let header = SourceContext {
            start_line: 0,
            code: "impl\n    Project {".into(),
        };
        let mut source = SourceDocument {
            symbol: Symbol::file("project.rs".into(), range),
            code: "    fn options() {}".into(),
            code_start: None,
            tokens: vec![],
            context: vec![SourceContext {
                start_line: 0,
                code: "impl\n    Project {\n    fn first() {}\n\n\n".into(),
            }],
            folded: vec![],
            expanded: vec![],
        };
        let original_code = source.context[0].code.clone();
        source.recover_expanded_context(std::slice::from_ref(&header));
        source.recover_expanded_context(std::slice::from_ref(&header));
        assert_eq!(source.context[0].code, original_code);
        assert_eq!(source.expanded.len(), 1);
        assert_eq!(source.expanded[0].start_line, 2);
        assert_eq!(source.expanded[0].code, "    fn first() {}\n\n\n");
        assert_eq!(source.display_lines()[2].fold, Some(0));
        source.validate().unwrap();
        source.expanded[0].code = "    changed();\n\n\n".into();
        assert!(source.validate().is_err());

        source.expanded.clear();
        source.context[0] = header.clone();
        source.recover_expanded_context(std::slice::from_ref(&header));
        assert!(source.expanded.is_empty()); // Multiline declarations are not revealed gaps.
        source.context[0].code = original_code;
        source.recover_expanded_context(&[SourceContext {
            start_line: 0,
            code: "impl ChangedProject {".into(),
        }]);
        assert!(source.expanded.is_empty()); // Changed declarations cannot rewrite saved text.
    }

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
