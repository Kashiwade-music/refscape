//! Contracts implemented by language backends and persistence adapters.

use crate::Result;
use refscape_model::{
    Position, ProjectCrate, ProjectOptions, Session, SourceDocument, SourceRange, Symbol,
};
use std::path::{Path, PathBuf};

/// All structure and relationships come from a language's official backend.
pub trait LanguageService: Send {
    fn open_project(&mut self, root: &Path, options: &ProjectOptions) -> Result<()>;

    /// Effective options after opening, including any automatically selected settings.
    fn project_options(&self) -> ProjectOptions {
        ProjectOptions::default()
    }

    fn files(&mut self) -> Result<Vec<PathBuf>>;
    fn symbols(&mut self, path: &Path) -> Result<Vec<Symbol>>;
    fn source(&mut self, symbol: &Symbol) -> Result<SourceDocument>;
    fn definitions(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>>;
    fn references(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>>;

    fn type_definitions(&mut self, _path: &Path, _position: Position) -> Result<Vec<Symbol>> {
        Ok(Vec::new())
    }

    fn document_highlights(
        &mut self,
        _path: &Path,
        _position: Position,
    ) -> Result<Vec<SourceRange>> {
        Ok(Vec::new())
    }

    fn hover(&mut self, _path: &Path, _position: Position) -> Result<Option<String>> {
        Ok(None)
    }

    fn project_crates(&mut self) -> Result<Vec<ProjectCrate>> {
        Ok(Vec::new())
    }

    fn search(&mut self, query: &str) -> Result<Vec<Symbol>> {
        let query = query.to_lowercase();
        let mut found = Vec::new();
        for path in self.files()? {
            collect_matches(&self.symbols(&path)?, &query, &mut found);
        }
        Ok(found)
    }
}

pub trait SessionRepository: Send {
    fn save(&self, path: &Path, session: &Session) -> Result<()>;
    fn load(&self, path: &Path) -> Result<Session>;
}

fn collect_matches(symbols: &[Symbol], query: &str, found: &mut Vec<Symbol>) {
    for symbol in symbols {
        if symbol.name.to_lowercase().contains(query) {
            found.push(symbol.clone());
        }
        collect_matches(&symbol.children, query, found);
    }
}
