//! Selects and owns the active language backend.
use refscape_application::ports::LanguageService;
use refscape_language_cpp::Clangd;
use refscape_language_python::Pyright;
use refscape_language_rust::RustAnalyzer;
use refscape_language_typescript::TypeScript;
use refscape_model::{
    Position, ProjectCrate, ProjectLanguage, ProjectOptions, SourceDocument, SourceRange, Symbol,
};
use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct LanguageBackend {
    rust_analyzer: PathBuf,
    clangd: PathBuf,
    typescript: PathBuf,
    pyright: PathBuf,
    timeout: Duration,
    active: Option<Box<dyn LanguageService>>,
}
impl Default for LanguageBackend {
    fn default() -> Self {
        Self::new(
            executable("REFSCAPE_RUST_ANALYZER", "rust-analyzer"),
            executable("REFSCAPE_CLANGD", "clangd"),
        )
    }
}
fn executable(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback.into())
}
impl LanguageBackend {
    pub fn new(rust_analyzer: impl Into<PathBuf>, clangd: impl Into<PathBuf>) -> Self {
        Self {
            rust_analyzer: rust_analyzer.into(),
            clangd: clangd.into(),
            typescript: executable(
                "REFSCAPE_TYPESCRIPT_LANGUAGE_SERVER",
                "typescript-language-server",
            ),
            pyright: executable("REFSCAPE_PYRIGHT", "basedpyright-langserver"),
            timeout: Duration::from_secs(120),
            active: None,
        }
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    pub fn with_typescript_server(mut self, executable: impl Into<PathBuf>) -> Self {
        self.typescript = executable.into();
        self
    }
    pub fn with_pyright_server(mut self, executable: impl Into<PathBuf>) -> Self {
        self.pyright = executable.into();
        self
    }
    fn session(&mut self) -> Result<&mut (dyn LanguageService + '_), String> {
        match self.active.as_mut() {
            Some(active) => Ok(active.as_mut()),
            None => Err("open a project before requesting analysis".into()),
        }
    }
}
fn select_language(root: &Path, options: &ProjectOptions) -> Result<ProjectLanguage, String> {
    match options.language {
        ProjectLanguage::Rust | ProjectLanguage::TypeScript | ProjectLanguage::Python
            if options.compilation_database.is_some() =>
        {
            Err("A compilation database applies to C/C++; choose the C/C++ language".into())
        }
        ProjectLanguage::Rust
        | ProjectLanguage::Cpp
        | ProjectLanguage::TypeScript
        | ProjectLanguage::Python => Ok(options.language),
        ProjectLanguage::Auto if options.compilation_database.is_some() => Ok(ProjectLanguage::Cpp),
        ProjectLanguage::Auto if refscape_language_rust::supports(root) => {
            Ok(ProjectLanguage::Rust)
        }
        ProjectLanguage::Auto if refscape_language_typescript::supports(root)? => {
            Ok(ProjectLanguage::TypeScript)
        }
        ProjectLanguage::Auto if refscape_language_python::supports(root)? => {
            Ok(ProjectLanguage::Python)
        }
        ProjectLanguage::Auto if refscape_language_cpp::supports(root)? => Ok(ProjectLanguage::Cpp),
        ProjectLanguage::Auto => Err(format!(
            "Cannot detect a Rust, C/C++, TypeScript/JavaScript, or Python project in {}. Select a source folder or choose its language explicitly",
            root.display()
        )),
    }
}
impl LanguageService for LanguageBackend {
    fn open_project(&mut self, root: &Path, options: &ProjectOptions) -> Result<(), String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("cannot open {}: {e}", root.display()))?;
        if !root.is_dir() {
            return Err(format!("{} is not a source folder", root.display()));
        }
        let mut next: Box<dyn LanguageService> = match select_language(&root, options)? {
            ProjectLanguage::Rust => {
                Box::new(RustAnalyzer::new(&self.rust_analyzer).with_timeout(self.timeout))
            }
            ProjectLanguage::Cpp => Box::new(Clangd::new(&self.clangd).with_timeout(self.timeout)),
            ProjectLanguage::TypeScript => {
                Box::new(TypeScript::new(&self.typescript).with_timeout(self.timeout))
            }
            ProjectLanguage::Python => {
                Box::new(Pyright::new(&self.pyright).with_timeout(self.timeout))
            }
            ProjectLanguage::Auto => unreachable!("selection always resolves automatic detection"),
        };
        next.open_project(&root, options)?;
        self.active = Some(next);
        Ok(())
    }
    fn project_options(&self) -> ProjectOptions {
        self.active
            .as_ref()
            .map(|active| active.project_options())
            .unwrap_or_default()
    }
    fn project_crates(&mut self) -> Result<Vec<ProjectCrate>, String> {
        self.session()?.project_crates()
    }
    fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        self.session()?.files()
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

#[cfg(test)]
mod tests;
