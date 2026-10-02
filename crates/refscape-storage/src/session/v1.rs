//! Version 1 wire schema. All serialization policy belongs to storage.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectLanguage {
    #[default]
    Auto,
    Rust,
    Cpp,
    #[serde(rename = "typescript")]
    TypeScript,
    Python,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectOptions {
    #[serde(default)]
    pub language: ProjectLanguage,
    #[serde(default)]
    pub compilation_database: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRange {
    pub start: Position,
    pub end: Position,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_fingerprint: Option<DocumentFingerprint>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentFingerprint {
    pub byte_len: u64,
    pub hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceContext {
    pub start_line: u32,
    pub code: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub offset: Point,
    pub zoom: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeCard {
    pub id: String,
    pub source: SourceDocument,
    pub position: Point,
    pub width: f32,
    pub height: f32,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionDocument {
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
