use crate::{
    ImportedSession, PersistableSession, Result,
    editing::{EditBasis, PreparedEdit, ValidatedCanvasPatch},
    state::{ApplicationSnapshot, VariableInspection},
};
use refscape_model::{
    CardId, CardSource, ConnectionKind, JobId, OperationContext, Point, Position, ProjectCrate,
    ProjectEpoch, ProjectOpenOptions, Symbol, Theme,
};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub enum ProjectRequest {
    Fresh {
        root: PathBuf,
        options: ProjectOpenOptions,
        destination: PathBuf,
    },
    Saved {
        path: PathBuf,
        expected_root: Option<PathBuf>,
        overrides: ProjectOpenOptions,
    },
    Loaded {
        loaded: Box<ImportedSession>,
        destination: PathBuf,
        expected_root: Option<PathBuf>,
        overrides: ProjectOpenOptions,
    },
}

#[derive(Clone)]
pub enum AnalysisQuery {
    RefreshSources(Arc<Vec<refscape_model::CodeCard>>),
    Files,
    Symbols(PathBuf),
    Search(String),
    Source {
        symbol: Symbol,
        position: Point,
        list_symbols: bool,
    },
    Navigate {
        card: CardId,
        path: PathBuf,
        position: Position,
        kind: ConnectionKind,
        anchor: Point,
        existing: Vec<CardSource>,
    },
    Fold {
        card: CardId,
        index: usize,
        source: CardSource,
    },
    Hover {
        card: CardId,
        source: CardSource,
        position: Position,
    },
    Inspect {
        card: CardId,
        source: CardSource,
        position: Position,
    },
}

pub enum AnalysisReply {
    Unchanged,
    Files(Vec<PathBuf>),
    Symbols(Vec<Symbol>),
    Search(Vec<Symbol>),
    Edit {
        edit: PreparedEdit,
        symbols: Option<Vec<Symbol>>,
    },
    Hover {
        card: CardId,
        source: CardSource,
        position: Position,
        value: Option<String>,
    },
    Inspection {
        card: CardId,
        source: CardSource,
        value: Option<VariableInspection>,
    },
}

pub struct PreparedApplicationProject {
    pub options: refscape_model::ResolvedProjectOptions,
    pub snapshot: ApplicationSnapshot,
    pub destination: PathBuf,
    pub crates: Vec<ProjectCrate>,
    pub files: Vec<PathBuf>,
    pub protection: Option<String>,
    pub listing_failed: bool,
    pub refreshed: bool,
}

pub enum Effect {
    PrepareProject {
        context: OperationContext,
        request: ProjectRequest,
        theme: Box<Theme>,
    },
    QueryAnalysis {
        context: OperationContext,
        basis: EditBasis,
        query: AnalysisQuery,
    },
    PlanCanvas {
        context: OperationContext,
        basis: EditBasis,
        snapshot: Arc<ApplicationSnapshot>,
        crates: Arc<Vec<ProjectCrate>>,
        edit: PreparedEdit,
        interaction: u64,
    },
    WriteSession {
        context: OperationContext,
        path: PathBuf,
        snapshot: PersistableSession,
    },
    CancelJob {
        id: JobId,
    },
    DisposeProject {
        epoch: ProjectEpoch,
    },
    CloseWindow,
}

pub enum Completion {
    ProjectPrepared {
        context: OperationContext,
        request: ProjectRequest,
        result: Box<Result<PreparedApplicationProject>>,
    },
    AnalysisQueried {
        context: OperationContext,
        basis: EditBasis,
        result: Result<AnalysisReply>,
        metadata: Option<Box<refscape_analysis::AnalysisMetadata>>,
    },
    CanvasPlanned {
        context: OperationContext,
        basis: EditBasis,
        edit: PreparedEdit,
        interaction: u64,
        result: Result<ValidatedCanvasPatch>,
    },
    SessionWritten {
        context: OperationContext,
        path: PathBuf,
        epoch: u64,
        revision: u64,
        result: Result<()>,
    },
    Disposed {
        epoch: ProjectEpoch,
    },
    Cancelled {
        id: JobId,
    },
    WindowClosed,
}
