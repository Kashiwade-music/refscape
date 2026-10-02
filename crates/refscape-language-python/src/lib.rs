//! Python and stub analysis through basedpyright, with Microsoft Pyright compatibility.
mod project;
mod server;

use refscape_application::ports::LanguageService;
use refscape_lsp::{LspSession, ServerConfiguration};
use refscape_model::{
    Position, ProjectCrate, ProjectLanguage, ProjectOptions, SourceDocument, SourceRange, Symbol,
};
use serde_json::json;
use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
    time::Duration,
};

pub use project::supports;

pub struct Python {
    executable: PathBuf,
    timeout: Duration,
    active: Option<ActiveProject>,
}

/// Compatibility name for callers configuring the Pyright-family server.
pub type Pyright = Python;

struct ActiveProject {
    root: PathBuf,
    session: LspSession,
}

impl Default for Python {
    fn default() -> Self {
        Self::new(
            env::var_os("REFSCAPE_PYRIGHT")
                .map(PathBuf::from)
                .unwrap_or_else(|| "basedpyright-langserver".into()),
        )
    }
}

impl Python {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            timeout: Duration::from_secs(120),
            active: None,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn session(&mut self) -> Result<&mut LspSession, String> {
        self.active
            .as_mut()
            .map(|active| &mut active.session)
            .ok_or_else(|| "open a project before requesting analysis".into())
    }

    fn index_documents(&mut self, query: Option<&str>) -> Result<Vec<Symbol>, String> {
        let mut output = Vec::new();
        // Pyright workspace symbols visit already parsed files and omit empty queries.
        // Ask its document-symbol provider to parse every source before workspace requests.
        for file in self.files()? {
            let symbols = self.symbols(&file)?;
            if let Some(query) = query {
                collect_matches(&symbols, query, &mut output);
            }
        }
        Ok(output)
    }
}

fn collect_matches(symbols: &[Symbol], query: &str, output: &mut Vec<Symbol>) {
    for symbol in symbols {
        if symbol.name.to_lowercase().contains(query) {
            output.push(symbol.clone());
        }
        collect_matches(&symbol.children, query, output);
    }
}

impl LanguageService for Python {
    fn open_project(&mut self, root: &Path, options: &ProjectOptions) -> Result<(), String> {
        if !matches!(
            options.language,
            ProjectLanguage::Auto | ProjectLanguage::Python
        ) || options.compilation_database.is_some()
        {
            return Err(
                "Python analyzes Python projects; compilation databases apply only to C/C++".into(),
            );
        }
        let root = root
            .canonicalize()
            .map_err(|error| format!("cannot open {}: {error}", root.display()))?;
        if !root.is_dir() {
            return Err(format!("{} is not a source folder", root.display()));
        }
        if options.language == ProjectLanguage::Auto && !supports(&root)? {
            return Err(format!(
                "{} does not contain a Python project",
                root.display()
            ));
        }
        let mut command = server::command(&root, &self.executable)?;
        let configuration = ServerConfiguration {
            name: "Python language server".into(),
            installation_hint: "Install basedpyright with pip install basedpyright or npm install -g basedpyright, or set REFSCAPE_PYRIGHT to a language-server executable or JavaScript entry point (REFSCAPE_NODE selects Node.js)".into(),
            initialization_options: json!({}),
            experimental_capabilities: json!({}),
            language_id: project::language_id,
            behavior: Box::new(server::PyrightBehavior),
        };
        let mut session =
            LspSession::start(root.clone(), &mut command, self.timeout, configuration)?;
        if let Some(seed) = project::files(&root)?.first() {
            session.symbols(seed)?;
        }
        self.active = Some(ActiveProject { root, session });
        Ok(())
    }

    fn project_options(&self) -> ProjectOptions {
        if self.active.is_some() {
            ProjectOptions {
                language: ProjectLanguage::Python,
                compilation_database: None,
            }
        } else {
            ProjectOptions::default()
        }
    }

    fn project_crates(&mut self) -> Result<Vec<ProjectCrate>, String> {
        Ok(vec![])
    }

    fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        project::files(&self.active.as_ref().ok_or("no project open")?.root)
    }

    fn search(&mut self, query: &str) -> Result<Vec<Symbol>, String> {
        let mut symbols = self.index_documents(Some(&query.to_lowercase()))?;
        symbols.extend(self.session()?.search(query)?);
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
        Ok(symbols)
    }

    fn symbols(&mut self, path: &Path) -> Result<Vec<Symbol>, String> {
        self.session()?.symbols(path)
    }

    fn source(&mut self, symbol: &Symbol) -> Result<SourceDocument, String> {
        self.session()?.source(symbol)
    }

    fn definitions(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.session()?.definitions(path, position)
    }

    fn references(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.index_documents(None)?;
        self.session()?.references(path, position)
    }

    fn type_definitions(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.session()?.type_definitions(path, position)
    }

    fn document_highlights(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<SourceRange>, String> {
        self.session()?.document_highlights(path, position)
    }

    fn hover(&mut self, path: &Path, position: Position) -> Result<Option<String>, String> {
        self.session()?.hover(path, position)
    }
}

#[cfg(test)]
mod tests;
