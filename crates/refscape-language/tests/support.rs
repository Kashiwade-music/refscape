//! Test harness owns a prepared session exactly as the application does.
#![allow(dead_code)]
use refscape_analysis::{AnalysisFactory, AnalysisSession, FeatureResult, OperationContext};
use refscape_model::{
    Position, ProjectCrate, ProjectOpenOptions, SourceDocument, SourceRange, Symbol,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
pub struct Opened<F: AnalysisFactory> {
    factory: F,
    session: Option<Box<dyn AnalysisSession>>,
    timeout: Duration,
}
impl<F: AnalysisFactory> Opened<F> {
    pub fn new(factory: F) -> Self {
        Self {
            factory,
            session: None,
            timeout: Duration::from_secs(120),
        }
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    pub fn open_project(
        &mut self,
        root: &Path,
        options: &ProjectOpenOptions,
    ) -> Result<(), String> {
        let prepared = self
            .factory
            .prepare(root, options, &OperationContext::detached(self.timeout))
            .map_err(|e| e.to_string())?;
        if let Some(previous) = self.session.replace(prepared.session) {
            previous
                .dispose(&OperationContext::detached(self.timeout))
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
    pub fn project_options(&self) -> ProjectOpenOptions {
        self.session
            .as_ref()
            .map(|session| session.project_options().to_open_options())
            .unwrap_or_default()
    }
    pub fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        let ctx = OperationContext::detached(self.timeout);
        self.session
            .as_mut()
            .ok_or("open a project")?
            .files(&ctx)
            .map_err(|e| e.to_string())
    }
    pub fn project_crates(&mut self) -> Result<Vec<ProjectCrate>, String> {
        let ctx = OperationContext::detached(self.timeout);
        self.session
            .as_mut()
            .ok_or("open a project")?
            .project_crates(&ctx)
            .map_err(|e| e.to_string())
    }
    pub fn symbols(&mut self, path: &Path) -> Result<Vec<Symbol>, String> {
        let ctx = OperationContext::detached(self.timeout);
        self.session
            .as_mut()
            .ok_or("open a project")?
            .symbols(path, &ctx)
            .map_err(|e| e.to_string())
    }
    pub fn source(&mut self, symbol: &Symbol) -> Result<SourceDocument, String> {
        let ctx = OperationContext::detached(self.timeout);
        self.session
            .as_mut()
            .ok_or("open a project")?
            .source(symbol, &ctx)
            .map_err(|e| e.to_string())
    }
    pub fn search(&mut self, query: &str) -> Result<Vec<Symbol>, String> {
        let ctx = OperationContext::detached(self.timeout);
        self.session
            .as_mut()
            .ok_or("open a project")?
            .search(query, &ctx)
            .map_err(|e| e.to_string())
    }
    pub fn definitions(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        let ctx = OperationContext::detached(self.timeout);
        self.session
            .as_mut()
            .ok_or("open a project")?
            .definitions(path, position, &ctx)
            .map(|targets| targets.into_iter().map(|target| target.symbol).collect())
            .map_err(|e| e.to_string())
    }
    pub fn references(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        let ctx = OperationContext::detached(self.timeout);
        self.session
            .as_mut()
            .ok_or("open a project")?
            .references(path, position, &ctx)
            .map(|targets| targets.into_iter().map(|target| target.symbol).collect())
            .map_err(|e| e.to_string())
    }
    pub fn type_definitions(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Symbol>, String> {
        let ctx = OperationContext::detached(self.timeout);
        match self
            .session
            .as_mut()
            .ok_or("open a project")?
            .type_definitions(path, position, &ctx)
            .map_err(|e| e.to_string())?
        {
            FeatureResult::Supported(value) => {
                Ok(value.into_iter().map(|target| target.symbol).collect())
            }
            FeatureResult::Unsupported => Ok(vec![]),
        }
    }
    pub fn document_highlights(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<SourceRange>, String> {
        let ctx = OperationContext::detached(self.timeout);
        match self
            .session
            .as_mut()
            .ok_or("open a project")?
            .document_highlights(path, position, &ctx)
            .map_err(|e| e.to_string())?
        {
            FeatureResult::Supported(value) => Ok(value),
            FeatureResult::Unsupported => Ok(vec![]),
        }
    }
    pub fn hover(&mut self, path: &Path, position: Position) -> Result<Option<String>, String> {
        let ctx = OperationContext::detached(self.timeout);
        match self
            .session
            .as_mut()
            .ok_or("open a project")?
            .hover(path, position, &ctx)
            .map_err(|e| e.to_string())?
        {
            FeatureResult::Supported(value) => Ok(value),
            FeatureResult::Unsupported => Ok(None),
        }
    }
}

impl<F: AnalysisFactory> Drop for Opened<F> {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            session
                .dispose(&OperationContext::detached(Duration::from_secs(5)))
                .expect("analysis process disposal");
        }
    }
}
