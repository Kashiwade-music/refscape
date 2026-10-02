use crate::ImportedSession;
use refscape_model::{
    ConnectionKind, Point, Position, ProjectOpenOptions, Symbol, Theme, Viewport,
};
use std::path::PathBuf;
#[derive(Clone, Copy, Debug)]
pub enum NavigationMode {
    Normal,
    Definition,
    References,
}
// Inputs are short-lived stack values; all card and graph payloads remain shared.
#[allow(clippy::large_enum_variant)]
pub enum Command {
    OpenProject {
        root: PathBuf,
        options: ProjectOpenOptions,
        destination: PathBuf,
    },
    OpenSession {
        path: PathBuf,
        expected_root: Option<PathBuf>,
        overrides: ProjectOpenOptions,
    },
    OpenLoaded {
        loaded: ImportedSession,
        destination: PathBuf,
        expected_root: Option<PathBuf>,
        overrides: ProjectOpenOptions,
    },
    SetCompilationDatabase(PathBuf),
    AddFile {
        path: PathBuf,
        position: Point,
    },
    AddSymbol {
        symbol: Symbol,
        position: Point,
        toggle: bool,
    },
    Navigate {
        card: String,
        position: Position,
        kind: ConnectionKind,
        anchor: Point,
        toggle: bool,
    },
    Click {
        card: String,
        position: Position,
        anchor: Point,
        mode: NavigationMode,
    },
    ToggleFold {
        card: String,
        index: usize,
        expand: bool,
    },
    MoveCard {
        id: String,
        position: Point,
    },
    CloseCard {
        id: String,
    },
    Arrange {
        selected: Option<String>,
    },
    UndoLayout,
    Pan(Point),
    Zoom {
        factor: f32,
        anchor: Point,
    },
    Fit {
        width: f32,
        height: f32,
    },
    SetViewport(Viewport),
    SetTheme(Theme),
    Files,
    RefreshSources,
    Symbols(PathBuf),
    Search(String),
    Hover {
        card: String,
        position: Position,
    },
    Inspect {
        card: String,
        position: Position,
    },
    CancelHover,
    CancelInspection,
    Save,
    SaveAs(PathBuf),
    RequestClose,
    Interaction {
        selected: Option<String>,
        dragging: bool,
    },
}
