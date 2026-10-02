//! Official-analysis contracts. Preparation produces a complete candidate; a session is always open.
pub use refscape_model::{ErrorKind, FeatureResult, OperationContext, RefscapeError};
use refscape_model::{
    Position, ProjectCrate, ProjectOpenOptions, ResolvedProjectOptions, SourceDocument,
    SourceRange, Symbol,
};
use std::path::{Path, PathBuf};

pub type AnalysisResult<T> = Result<T, RefscapeError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationLocation {
    pub document: PathBuf,
    pub target_range: SourceRange,
    pub selection_range: SourceRange,
    pub origin_range: Option<SourceRange>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationTarget {
    pub symbol: Symbol,
    pub location: NavigationLocation,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnalysisCapabilities {
    pub document_symbols: bool,
    pub workspace_symbols: bool,
    pub definitions: bool,
    pub references: bool,
    pub type_definitions: bool,
    pub highlights: bool,
    pub hover: bool,
    pub semantic_tokens: bool,
}

pub enum CatalogOutcome {
    Ready(Vec<PathBuf>),
    Failed(RefscapeError),
}

pub struct PreparedProject {
    pub root: PathBuf,
    pub options: ResolvedProjectOptions,
    pub catalog: CatalogOutcome,
    pub crates: Vec<ProjectCrate>,
    pub capabilities: AnalysisCapabilities,
    pub session: Box<dyn AnalysisSession>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisMetadata {
    pub options: ResolvedProjectOptions,
    pub crates: Vec<ProjectCrate>,
    pub catalog_revision: u64,
    pub files: Vec<PathBuf>,
}

pub trait AnalysisFactory: Send + Sync {
    fn prepare(
        &self,
        root: &Path,
        options: &ProjectOpenOptions,
        context: &OperationContext,
    ) -> AnalysisResult<PreparedProject>;
}

pub trait AnalysisSession: Send {
    /// Filesystem-backed sessions refresh saved and live sources from disk.
    /// In-memory/virtual backends can opt out without exposing fake file paths.
    fn supports_source_reload(&self) -> bool {
        true
    }
    /// Identity of the immutable document captured by the current operation.
    fn document_fingerprint(
        &mut self,
        _path: &Path,
        context: &OperationContext,
    ) -> AnalysisResult<Option<refscape_model::DocumentFingerprint>> {
        context.check()?;
        Ok(None)
    }
    /// Releases immutable captures after a composite effect's final port call.
    fn finish_operation(&mut self, _context: &OperationContext) {}
    /// Runs on the effect worker. Dropping a handle starts disposal; this explicit
    /// boundary also waits for backend resources to be reaped under the caller's budget.
    fn dispose(self: Box<Self>, context: &OperationContext) -> AnalysisResult<()> {
        drop(self);
        context.check()
    }
    fn project_options(&self) -> ResolvedProjectOptions;
    /// Published generation only: this accessor performs no filesystem work.
    fn metadata_snapshot(&self) -> Option<AnalysisMetadata> {
        None
    }
    fn capabilities(&self) -> AnalysisCapabilities {
        AnalysisCapabilities::default()
    }
    fn files(&mut self, context: &OperationContext) -> AnalysisResult<Vec<PathBuf>>;
    fn symbols(&mut self, path: &Path, context: &OperationContext) -> AnalysisResult<Vec<Symbol>>;
    fn source(
        &mut self,
        symbol: &Symbol,
        context: &OperationContext,
    ) -> AnalysisResult<SourceDocument>;
    fn definitions(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationTarget>>;
    fn references(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationTarget>>;
    fn type_definitions(
        &mut self,
        _path: &Path,
        _position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Vec<NavigationTarget>>> {
        context.check()?;
        Ok(FeatureResult::Unsupported)
    }
    fn document_highlights(
        &mut self,
        _path: &Path,
        _position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Vec<SourceRange>>> {
        context.check()?;
        Ok(FeatureResult::Unsupported)
    }
    fn hover(
        &mut self,
        _path: &Path,
        _position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Option<String>>> {
        context.check()?;
        Ok(FeatureResult::Unsupported)
    }
    fn project_crates(&mut self, context: &OperationContext) -> AnalysisResult<Vec<ProjectCrate>>;
    fn search(&mut self, query: &str, context: &OperationContext) -> AnalysisResult<Vec<Symbol>>;
}
