//! Cargo project discovery and rust-analyzer policy.
mod files;
mod project;
mod server;
use refscape_application::ports::LanguageService;
use refscape_lsp::{LspSession, ServerConfiguration};
use refscape_model::{
    Position, ProjectCrate, ProjectLanguage, ProjectOptions, SourceDocument, SourceRange, Symbol,
};
use serde_json::json;
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub struct RustAnalyzer {
    executable: PathBuf,
    timeout: Duration,
    active: Option<ActiveProject>,
}
struct ActiveProject {
    session: LspSession,
    project: project::Project,
}
/// Whether this root contains the Cargo manifest required by the Rust backend.
pub fn supports(root: &Path) -> bool {
    root.join("Cargo.toml").is_file()
}
impl Default for RustAnalyzer {
    fn default() -> Self {
        Self::new(
            env::var_os("REFSCAPE_RUST_ANALYZER")
                .map(PathBuf::from)
                .unwrap_or_else(|| "rust-analyzer".into()),
        )
    }
}
impl RustAnalyzer {
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
}
impl LanguageService for RustAnalyzer {
    fn open_project(&mut self, root: &Path, options: &ProjectOptions) -> Result<(), String> {
        if !matches!(
            options.language,
            ProjectLanguage::Auto | ProjectLanguage::Rust
        ) || options.compilation_database.is_some()
        {
            return Err("rust-analyzer analyzes Cargo projects and does not accept C/C++ compilation databases".into());
        }
        let root = root
            .canonicalize()
            .map_err(|e| format!("cannot open {}: {e}", root.display()))?;
        if !supports(&root) {
            return Err(format!(
                "{} is not a Cargo project (Cargo.toml is missing)",
                root.display()
            ));
        }
        let project = project::Project::discover(&root, self.timeout)?;
        let mut command = Command::new(&self.executable);
        command.current_dir(&root);
        let session = LspSession::start(root, &mut command, self.timeout, ServerConfiguration {
            name: "rust-analyzer".into(),
            installation_hint: "Install `rustup component add rust-analyzer` or set REFSCAPE_RUST_ANALYZER to its executable".into(),
            initialization_options: json!({"checkOnSave":false}),
            experimental_capabilities: json!({"serverStatusNotification":true}),
            language_id: |_| "rust",
            behavior: Box::<server::RustServer>::default(),
        })?;
        self.active = Some(ActiveProject { session, project });
        Ok(())
    }
    fn project_options(&self) -> ProjectOptions {
        self.active
            .as_ref()
            .map(|_| ProjectOptions {
                language: ProjectLanguage::Rust,
                compilation_database: None,
            })
            .unwrap_or_default()
    }
    fn project_crates(&mut self) -> Result<Vec<ProjectCrate>, String> {
        Ok(self
            .active
            .as_ref()
            .ok_or("no project open")?
            .project
            .crates
            .clone())
    }
    fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        let project = &self.active.as_ref().ok_or("no project open")?.project;
        let mut files = vec![];
        for package in &project.crates {
            files::collect_files(&package.root, &mut files)?;
        }
        files.extend(project.targets.iter().cloned());
        files.sort();
        files.dedup();
        Ok(files)
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
    fn search(&mut self, query: &str) -> Result<Vec<Symbol>, String> {
        self.session()?.search(query)
    }
}
