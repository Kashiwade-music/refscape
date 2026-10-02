use crate::catalog::CatalogSnapshot;
use refscape_analysis::{
    AnalysisCapabilities, AnalysisResult, AnalysisSession, CatalogOutcome, FeatureResult,
    OperationContext, PreparedProject, RefscapeError,
};
use refscape_lsp::{LspProjectSession, ServerConfiguration};
use refscape_model::{
    Position, ProjectCrate, ResolvedProjectOptions, SourceDocument, SourceRange, Symbol,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMergePolicy {
    WorkspaceOnly,
    WorkspaceFirst,
    DocumentsFirst,
}
pub struct Metadata {
    pub options: ResolvedProjectOptions,
    pub files: Vec<PathBuf>,
    pub catalog_error: Option<RefscapeError>,
    pub crates: Vec<ProjectCrate>,
    pub seed: Option<PathBuf>,
    pub search: SearchMergePolicy,
    pub prewarm_references: bool,
    /// Metadata inputs are refreshed together, even when source paths did not change.
    pub revision: u64,
}
pub trait MetadataProvider: Send {
    fn refresh(&mut self, context: &OperationContext) -> AnalysisResult<Metadata>;
}
pub struct LspAnalysisSession {
    lsp: LspProjectSession,
    provider: Box<dyn MetadataProvider>,
    metadata: Metadata,
    catalog: CatalogSnapshot,
    symbols: BTreeMap<PathBuf, Vec<Symbol>>,
    index_epoch: u64,
    published_revision: u64,
}
impl LspAnalysisSession {
    pub fn prepare(
        root: PathBuf,
        mut command: Command,
        configuration: ServerConfiguration,
        mut provider: Box<dyn MetadataProvider>,
        context: &OperationContext,
    ) -> AnalysisResult<PreparedProject> {
        context.check()?;
        let mut metadata = provider.refresh(context)?;
        let catalog = match CatalogSnapshot::default()
            .refresh_with_context(metadata.files.clone(), context)
        {
            Ok((catalog, _)) => catalog,
            Err(error) if error.kind == refscape_analysis::ErrorKind::Io => {
                metadata.catalog_error = Some(error);
                CatalogSnapshot::default()
            }
            Err(error) => return Err(error),
        };
        let mut lsp = LspProjectSession::start(root.clone(), &mut command, context, configuration)?;
        let mut symbols = BTreeMap::new();
        if let Some(seed) = &metadata.seed {
            symbols.insert(seed.clone(), lsp.symbols(seed, context)?);
        }
        lsp.finish_operation(context);
        let capabilities = lsp.capabilities();
        let index_epoch = lsp.analysis_epoch();
        let options = metadata.options.clone();
        let crates = metadata.crates.clone();
        let files = catalog.ordered_files.clone();
        let metadata_error = metadata.catalog_error.clone();
        let session = Self {
            lsp,
            provider,
            metadata,
            catalog,
            symbols,
            index_epoch,
            published_revision: 1,
        };
        Ok(PreparedProject {
            root,
            options,
            catalog: match &metadata_error {
                Some(error) => CatalogOutcome::Failed(error.clone()),
                None => CatalogOutcome::Ready(files),
            },
            crates,
            capabilities,
            session: Box::new(session),
        })
    }
    fn refresh(&mut self, context: &OperationContext) -> AnalysisResult<()> {
        context.check()?;
        let metadata = self.provider.refresh(context)?;
        if let Some(error) = &metadata.catalog_error {
            return Err(error.clone());
        }
        let (catalog, delta) = self
            .catalog
            .refresh_with_context(metadata.files.clone(), context)?;
        let published_revision = self
            .published_revision
            .checked_add(u64::from(
                catalog.revision != self.catalog.revision
                    || metadata.revision != self.metadata.revision,
            ))
            .ok_or_else(|| {
                RefscapeError::new(
                    refscape_analysis::ErrorKind::InternalInvariant,
                    "Analysis metadata revision overflow",
                )
            })?;
        self.lsp.close_documents(&delta.removed, context)?;
        for path in delta.removed.iter().chain(&delta.changed) {
            self.symbols.remove(path);
        }
        if metadata.revision != self.metadata.revision {
            self.lsp.invalidate_analysis();
        }
        let epoch = self.lsp.analysis_epoch();
        if epoch != self.index_epoch || metadata.revision != self.metadata.revision {
            self.symbols.clear();
        }
        context.check()?;
        self.index_epoch = epoch;
        self.metadata = metadata;
        self.catalog = catalog;
        self.published_revision = published_revision;
        Ok(())
    }
    fn prewarm(&mut self, context: &OperationContext) -> AnalysisResult<()> {
        for file in &self.catalog.ordered_files {
            context.check()?;
            if !self.symbols.contains_key(file) {
                let symbols = self.lsp.symbols(file, context)?;
                self.symbols.insert(file.clone(), symbols);
            }
        }
        Ok(())
    }
}
/// Stable sorting preserves the first provider for equal-ranked duplicate IDs.
pub fn merge_search(
    workspace: Vec<Symbol>,
    documents: Vec<Symbol>,
    policy: SearchMergePolicy,
) -> Vec<Symbol> {
    if policy == SearchMergePolicy::WorkspaceOnly {
        return workspace;
    }
    let mut symbols = if policy == SearchMergePolicy::DocumentsFirst {
        let mut values = documents;
        values.extend(workspace);
        values
    } else {
        let mut values = workspace;
        values.extend(documents);
        values
    };
    symbols.sort_by(|left, right| {
        (&left.path, left.range.start, left.range.end, &left.name).cmp(&(
            &right.path,
            right.range.start,
            right.range.end,
            &right.name,
        ))
    });
    let mut seen = BTreeSet::new();
    symbols.retain(|symbol| seen.insert(symbol.id.clone()));
    symbols
}
pub fn collect_matches(symbols: &[Symbol], query: &str, output: &mut Vec<Symbol>) {
    let mut stack: Vec<_> = symbols.iter().rev().collect();
    while let Some(symbol) = stack.pop() {
        if symbol.name.to_lowercase().contains(query) {
            output.push(symbol.clone());
        }
        stack.extend(symbol.children.iter().rev());
    }
}
impl AnalysisSession for LspAnalysisSession {
    fn document_fingerprint(
        &mut self,
        path: &Path,
        context: &OperationContext,
    ) -> AnalysisResult<Option<refscape_model::DocumentFingerprint>> {
        self.lsp.document_fingerprint(path, context).map(Some)
    }
    fn dispose(self: Box<Self>, context: &OperationContext) -> AnalysisResult<()> {
        let disposal = self.lsp.disposal();
        drop(self);
        disposal.wait(context)
    }
    fn project_options(&self) -> ResolvedProjectOptions {
        self.metadata.options.clone()
    }
    fn finish_operation(&mut self, context: &OperationContext) {
        self.lsp.finish_operation(context);
    }
    fn metadata_snapshot(&self) -> Option<refscape_analysis::AnalysisMetadata> {
        Some(refscape_analysis::AnalysisMetadata {
            options: self.metadata.options.clone(),
            crates: self.metadata.crates.clone(),
            catalog_revision: self.published_revision,
            files: self.catalog.ordered_files.clone(),
        })
    }
    fn capabilities(&self) -> AnalysisCapabilities {
        self.lsp.capabilities()
    }
    fn files(&mut self, context: &OperationContext) -> AnalysisResult<Vec<PathBuf>> {
        self.refresh(context)?;
        Ok(self.catalog.ordered_files.clone())
    }
    fn project_crates(&mut self, context: &OperationContext) -> AnalysisResult<Vec<ProjectCrate>> {
        self.refresh(context)?;
        Ok(self.metadata.crates.clone())
    }
    fn symbols(&mut self, path: &Path, context: &OperationContext) -> AnalysisResult<Vec<Symbol>> {
        self.lsp.symbols(path, context)
    }
    fn source(
        &mut self,
        symbol: &Symbol,
        context: &OperationContext,
    ) -> AnalysisResult<SourceDocument> {
        self.lsp.source(symbol, context)
    }
    fn definitions(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<refscape_analysis::NavigationTarget>> {
        self.lsp.definitions(path, position, context)
    }
    fn references(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<refscape_analysis::NavigationTarget>> {
        if self.metadata.prewarm_references {
            self.refresh(context)?;
            self.prewarm(context)?;
        }
        self.lsp.references(path, position, context)
    }
    fn type_definitions(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Vec<refscape_analysis::NavigationTarget>>> {
        self.lsp.type_definitions(path, position, context)
    }
    fn document_highlights(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Vec<SourceRange>>> {
        self.lsp.document_highlights(path, position, context)
    }
    fn hover(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Option<String>>> {
        self.lsp.hover(path, position, context)
    }
    fn search(&mut self, query: &str, context: &OperationContext) -> AnalysisResult<Vec<Symbol>> {
        self.refresh(context)?;
        let policy = self.metadata.search;
        // TS/Python require a parse barrier before workspace search; clangd preserves workspace-first execution.
        if self.metadata.prewarm_references {
            self.prewarm(context)?;
        }
        let workspace = self.lsp.search(query, context)?;
        let mut documents = vec![];
        if policy != SearchMergePolicy::WorkspaceOnly {
            self.prewarm(context)?;
            for symbols in self.symbols.values() {
                collect_matches(symbols, &query.to_lowercase(), &mut documents);
            }
        }
        Ok(merge_search(workspace, documents, policy))
    }
}
