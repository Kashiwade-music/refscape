use super::{RustAnalyzer, cpp};
use refscape_application::LanguageService;
use refscape_model::{
    Position, ProjectCrate, ProjectLanguage, ProjectOptions, SourceDocument, SourceRange, Symbol,
};
use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

/// One persistent clangd process per opened C/C++ project, sharing the LSP adapter.
pub struct Clangd {
    inner: RustAnalyzer,
}

impl Default for Clangd {
    fn default() -> Self {
        Self::new(executable("REFSCAPE_CLANGD", "clangd"))
    }
}

impl Clangd {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            inner: RustAnalyzer::new(executable),
        }
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.inner.timeout = timeout;
        self
    }
    fn session(&mut self) -> Result<&mut RustAnalyzer, String> {
        Ok(&mut self.inner)
    }
}

fn executable(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback.into())
}

/// Routes each project to its official language server. Failed switches retain the
/// active server and its metadata, including when switching between languages.
pub struct LanguageBackend {
    rust_analyzer: PathBuf,
    clangd: PathBuf,
    timeout: Duration,
    active: Option<RustAnalyzer>,
}

impl Default for LanguageBackend {
    fn default() -> Self {
        Self::new(
            executable("REFSCAPE_RUST_ANALYZER", "rust-analyzer"),
            executable("REFSCAPE_CLANGD", "clangd"),
        )
    }
}

impl LanguageBackend {
    pub fn new(
        rust_analyzer_executable: impl Into<PathBuf>,
        clangd_executable: impl Into<PathBuf>,
    ) -> Self {
        Self {
            rust_analyzer: rust_analyzer_executable.into(),
            clangd: clangd_executable.into(),
            timeout: Duration::from_secs(120),
            active: None,
        }
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    fn session(&mut self) -> Result<&mut RustAnalyzer, String> {
        self.active
            .as_mut()
            .ok_or_else(|| "open a project before requesting analysis".into())
    }
}

// Both public front ends use the same document/navigation/token implementation.
macro_rules! delegate_analysis {
    () => {
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
        fn type_definitions(
            &mut self,
            path: &Path,
            position: Position,
        ) -> Result<Vec<Symbol>, String> {
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
        fn project_crates(&mut self) -> Result<Vec<ProjectCrate>, String> {
            self.session()?.project_crates()
        }
    };
}

impl LanguageService for Clangd {
    fn open_project(&mut self, root: &Path) -> Result<(), String> {
        self.open_project_with_options(
            root,
            &ProjectOptions {
                language: ProjectLanguage::Cpp,
                compilation_database: None,
            },
        )
    }
    fn open_project_with_options(
        &mut self,
        root: &Path,
        options: &ProjectOptions,
    ) -> Result<(), String> {
        if options.language == ProjectLanguage::Rust {
            return Err("clangd analyzes C/C++ projects; choose the C/C++ language".into());
        }
        let root = root
            .canonicalize()
            .map_err(|e| format!("cannot open {}: {e}", root.display()))?;
        if !root.is_dir() {
            return Err(format!("{} is not a source folder", root.display()));
        }
        let project = cpp::CppProject::discover(&root, options)?;
        self.inner.start_session(root, None, Some(project))
    }
    fn project_options(&self) -> ProjectOptions {
        self.inner
            .cpp_project
            .as_ref()
            .map(cpp::CppProject::options)
            .unwrap_or(ProjectOptions {
                language: ProjectLanguage::Cpp,
                compilation_database: None,
            })
    }
    delegate_analysis!();
}

impl LanguageService for LanguageBackend {
    fn open_project(&mut self, root: &Path) -> Result<(), String> {
        self.open_project_with_options(root, &ProjectOptions::default())
    }
    fn open_project_with_options(
        &mut self,
        root: &Path,
        options: &ProjectOptions,
    ) -> Result<(), String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("cannot open {}: {e}", root.display()))?;
        if !root.is_dir() {
            return Err(format!("{} is not a source folder", root.display()));
        }
        let language = cpp::detect_language(&root, options)?;
        let executable = if language == ProjectLanguage::Cpp {
            &self.clangd
        } else {
            &self.rust_analyzer
        };
        let mut next = RustAnalyzer::new(executable).with_timeout(self.timeout);
        if language == ProjectLanguage::Cpp {
            let project = cpp::CppProject::discover(&root, options)?;
            next.start_session(root, None, Some(project))?;
        } else {
            next.open_project(&root)?;
        }
        self.active = Some(next);
        Ok(())
    }
    fn project_options(&self) -> ProjectOptions {
        self.active
            .as_ref()
            .map(LanguageService::project_options)
            .unwrap_or_default()
    }
    delegate_analysis!();
}
