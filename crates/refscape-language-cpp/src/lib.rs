//! C/C++ project discovery and clangd policy.
mod project;
use refscape_application::ports::LanguageService;
use refscape_lsp::transport::DefaultServerBehavior;
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

pub struct Clangd {
    executable: PathBuf,
    timeout: Duration,
    active: Option<ActiveProject>,
}
struct ActiveProject {
    root: PathBuf,
    session: LspSession,
    project: project::CppProject,
}
pub fn supports(root: &Path) -> Result<bool, String> {
    project::supports(root)
}
impl Default for Clangd {
    fn default() -> Self {
        Self::new(
            env::var_os("REFSCAPE_CLANGD")
                .map(PathBuf::from)
                .unwrap_or_else(|| "clangd".into()),
        )
    }
}
impl Clangd {
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
impl LanguageService for Clangd {
    fn open_project(&mut self, root: &Path, options: &ProjectOptions) -> Result<(), String> {
        if options.language == ProjectLanguage::Auto
            && options.compilation_database.is_none()
            && !supports(root)?
        {
            return Err(format!(
                "{} does not contain a C/C++ project",
                root.display()
            ));
        }
        if !matches!(
            options.language,
            ProjectLanguage::Auto | ProjectLanguage::Cpp
        ) {
            return Err("clangd analyzes C/C++ projects; choose the C/C++ language".into());
        }
        let root = root
            .canonicalize()
            .map_err(|e| format!("cannot open {}: {e}", root.display()))?;
        if !root.is_dir() {
            return Err(format!("{} is not a source folder", root.display()));
        }
        let project = project::CppProject::discover(&root, options)?;
        let mut command = Command::new(&self.executable);
        command
            .current_dir(&root)
            .args(["--background-index", "--enable-config"]);
        if let Some(database) = &project.database {
            let mut flag = std::ffi::OsString::from("--compile-commands-dir=");
            flag.push(
                database
                    .parent()
                    .ok_or("compilation database has no directory")?,
            );
            command.arg(flag);
        }
        let mut session = LspSession::start(
            root.clone(),
            &mut command,
            self.timeout,
            ServerConfiguration {
                name: "clangd".into(),
                installation_hint: "Install clangd (LLVM) or set REFSCAPE_CLANGD to its executable"
                    .into(),
                initialization_options: json!({}),
                experimental_capabilities: json!({}),
                language_id: project::language_id,
                behavior: Box::new(DefaultServerBehavior),
            },
        )?;
        // clangd loads compile commands lazily. Await one TU's AST before publishing the session.
        if let Some(seed) = project.index_seed(&root)? {
            session.symbols(&seed)?;
        }
        self.active = Some(ActiveProject {
            root,
            session,
            project,
        });
        Ok(())
    }
    fn project_options(&self) -> ProjectOptions {
        self.active
            .as_ref()
            .map(|active| active.project.options())
            .unwrap_or_default()
    }
    fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        let active = self.active.as_ref().ok_or("no project open")?;
        active.project.files(&active.root)
    }
    fn project_crates(&mut self) -> Result<Vec<ProjectCrate>, String> {
        Ok(vec![])
    }
    fn search(&mut self, query: &str) -> Result<Vec<Symbol>, String> {
        let mut symbols = self.session()?.search(query)?;
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.project.database.is_none())
        {
            // Fallback commands lack background indexing; supplement with document symbols.
            fn matches(symbols: &[Symbol], query: &str, output: &mut Vec<Symbol>) {
                for symbol in symbols {
                    if symbol.name.to_lowercase().contains(query) {
                        output.push(symbol.clone());
                    }
                    matches(&symbol.children, query, output);
                }
            }
            let query = query.to_lowercase();
            for file in self.files()? {
                matches(&self.symbols(&file)?, &query, &mut symbols);
            }
            symbols.sort_by(|left, right| {
                (&left.path, left.range.start, left.range.end, &left.name).cmp(&(
                    &right.path,
                    right.range.start,
                    right.range.end,
                    &right.name,
                ))
            });
            let mut seen = std::collections::BTreeSet::new();
            symbols.retain(|symbol| seen.insert(symbol.id.clone()));
        }
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
