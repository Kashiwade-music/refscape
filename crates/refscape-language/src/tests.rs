#![allow(dead_code)]
use super::*;
use refscape_model::{Position, ProjectCrate, SourceDocument, SourceRange, Symbol};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "refscape-router-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn write(&self, name: &str) {
        fs::write(self.0.join(name), "").unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(
            self.0.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("refscape-router-")
        );
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn selection_handles_mixed_roots_explicit_overrides_and_invalid_options() {
    let fixture = Fixture::new();
    assert!(select_language(&fixture.0, &ProjectOpenOptions::default()).is_err());
    fixture.write("main.cpp");
    assert_eq!(
        select_language(&fixture.0, &ProjectOpenOptions::default()).unwrap(),
        ProjectLanguage::Cpp
    );
    fixture.write("Cargo.toml");
    assert_eq!(
        select_language(&fixture.0, &ProjectOpenOptions::default()).unwrap(),
        ProjectLanguage::Rust
    );
    let database = ProjectOpenOptions {
        compilation_database: Some("build".into()),
        ..ProjectOpenOptions::default()
    };
    assert_eq!(
        select_language(&fixture.0, &database).unwrap(),
        ProjectLanguage::Cpp
    );
    let cpp = ProjectOpenOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: None,
    };
    assert_eq!(
        select_language(&fixture.0, &cpp).unwrap(),
        ProjectLanguage::Cpp
    );
    let invalid = ProjectOpenOptions {
        language: ProjectLanguage::Rust,
        ..database
    };
    assert!(
        select_language(&fixture.0, &invalid)
            .unwrap_err()
            .contains("compilation database")
    );
}

#[test]
fn typescript_precedes_native_cpp_files_and_respects_explicit_language() {
    let fixture = Fixture::new();
    fixture.write("native.cpp");
    fixture.write("App.native.tsx");
    assert_eq!(
        select_language(&fixture.0, &ProjectOpenOptions::default()).unwrap(),
        ProjectLanguage::TypeScript
    );
    let explicit = ProjectOpenOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: None,
    };
    assert_eq!(
        select_language(&fixture.0, &explicit).unwrap(),
        ProjectLanguage::Cpp
    );
    let database = ProjectOpenOptions {
        compilation_database: Some("build".into()),
        ..ProjectOpenOptions::default()
    };
    assert_eq!(
        select_language(&fixture.0, &database).unwrap(),
        ProjectLanguage::Cpp
    );
    let invalid = ProjectOpenOptions {
        language: ProjectLanguage::TypeScript,
        ..database
    };
    assert!(select_language(&fixture.0, &invalid).is_err());
    fixture.write("Cargo.toml");
    assert_eq!(
        select_language(&fixture.0, &ProjectOpenOptions::default()).unwrap(),
        ProjectLanguage::Rust
    );
}

#[test]
fn failed_typescript_start_preserves_previous_project() {
    let fixture = Fixture::new();
    fixture.write("App.jsx");
    let mut router = Opened::new(LanguageBackend::default())
        .with_typescript_server(fixture.0.join("no-typescript-server"));
    router.session = Some(Box::new(ActiveRust));
    let error = router
        .open_project(&fixture.0, &ProjectOpenOptions::default())
        .unwrap_err();
    assert!(
        error.contains("REFSCAPE_TYPESCRIPT_LANGUAGE_SERVER"),
        "{error}"
    );
    assert_eq!(router.project_options().language, ProjectLanguage::Rust);
    assert_eq!(router.files().unwrap(), [PathBuf::from("active.rs")]);
}

#[test]
fn python_detection_follows_rust_and_typescript_and_precedes_cpp() {
    let fixture = Fixture::new();
    fixture.write("native.cpp");
    fixture.write("main.py");
    assert_eq!(
        select_language(&fixture.0, &ProjectOpenOptions::default()).unwrap(),
        ProjectLanguage::Python
    );
    let explicit = ProjectOpenOptions {
        language: ProjectLanguage::Python,
        compilation_database: None,
    };
    let database = ProjectOpenOptions {
        compilation_database: Some("build".into()),
        ..ProjectOpenOptions::default()
    };
    assert_eq!(
        select_language(&fixture.0, &database).unwrap(),
        ProjectLanguage::Cpp
    );
    assert!(
        select_language(
            &fixture.0,
            &ProjectOpenOptions {
                language: ProjectLanguage::Python,
                ..database
            }
        )
        .is_err()
    );
    fixture.write("App.tsx");
    assert_eq!(
        select_language(&fixture.0, &ProjectOpenOptions::default()).unwrap(),
        ProjectLanguage::TypeScript
    );
    fixture.write("Cargo.toml");
    assert_eq!(
        select_language(&fixture.0, &ProjectOpenOptions::default()).unwrap(),
        ProjectLanguage::Rust
    );
    assert_eq!(
        select_language(&fixture.0, &explicit).unwrap(),
        ProjectLanguage::Python
    );
}

#[test]
fn python_detection_ignores_javascript_bundled_in_virtual_environments() {
    let fixture = Fixture::new();
    fixture.write("main.py");
    for directory in [
        ".venv/Lib/site-packages/pkg/static",
        "custom-env/Lib/package/static",
    ] {
        fs::create_dir_all(fixture.0.join(directory)).unwrap();
        fixture.write(&format!("{directory}/client.js"));
    }
    fixture.write("custom-env/pyvenv.cfg");
    assert_eq!(
        select_language(&fixture.0, &ProjectOpenOptions::default()).unwrap(),
        ProjectLanguage::Python
    );
    fixture.write("main.ts");
    assert_eq!(
        select_language(&fixture.0, &ProjectOpenOptions::default()).unwrap(),
        ProjectLanguage::TypeScript
    );
}

#[test]
fn failed_python_start_preserves_previous_project_and_honors_server_override() {
    let fixture = Fixture::new();
    fixture.write("main.py");
    let mut router = Opened::new(LanguageBackend::default())
        .with_pyright_server(fixture.0.join("no-python-server"));
    router.session = Some(Box::new(ActiveRust));
    let error = router
        .open_project(&fixture.0, &ProjectOpenOptions::default())
        .unwrap_err();
    assert!(error.contains("REFSCAPE_PYRIGHT"), "{error}");
    assert_eq!(router.project_options().language, ProjectLanguage::Rust);
    assert_eq!(router.files().unwrap(), [PathBuf::from("active.rs")]);
}

#[test]
fn unavailable_python_server_leaves_router_without_an_active_project() {
    let fixture = Fixture::new();
    fixture.write("main.py");
    let mut router = Opened::new(LanguageBackend::default())
        .with_pyright_server(fixture.0.join("no-python-server"));
    let error = router
        .open_project(&fixture.0, &ProjectOpenOptions::default())
        .unwrap_err();
    assert!(error.contains("REFSCAPE_PYRIGHT"), "{error}");
    assert_eq!(router.project_options(), ProjectOpenOptions::default());
    assert!(router.files().unwrap_err().contains("open a project"));
}

struct ActiveRust;
impl refscape_analysis::AnalysisSession for ActiveRust {
    fn project_options(&self) -> refscape_model::ResolvedProjectOptions {
        ProjectOpenOptions {
            language: ProjectLanguage::Rust,
            compilation_database: None,
        }
        .try_into()
        .unwrap()
    }
    fn project_crates(&mut self, _: &OperationContext) -> AnalysisResult<Vec<ProjectCrate>> {
        Ok(vec![])
    }
    fn files(&mut self, _: &OperationContext) -> AnalysisResult<Vec<PathBuf>> {
        Ok(vec!["active.rs".into()])
    }
    fn symbols(&mut self, _: &Path, _: &OperationContext) -> AnalysisResult<Vec<Symbol>> {
        Ok(vec![])
    }
    fn source(&mut self, _: &Symbol, _: &OperationContext) -> AnalysisResult<SourceDocument> {
        Err("not needed".into())
    }
    fn definitions(
        &mut self,
        _: &Path,
        _: Position,
        _: &OperationContext,
    ) -> AnalysisResult<Vec<refscape_analysis::NavigationTarget>> {
        Ok(vec![])
    }
    fn references(
        &mut self,
        _: &Path,
        _: Position,
        _: &OperationContext,
    ) -> AnalysisResult<Vec<refscape_analysis::NavigationTarget>> {
        Ok(vec![])
    }
    fn search(&mut self, _: &str, _: &OperationContext) -> AnalysisResult<Vec<Symbol>> {
        Ok(vec![])
    }
}
#[test]
fn failed_cross_language_start_preserves_active_analysis_and_options() {
    let fixture = Fixture::new();
    fixture.write("main.cpp");
    let mut router = Opened::new(LanguageBackend::new(
        fixture.0.join("no-rust-analyzer"),
        fixture.0.join("no-clangd"),
    ));
    router.session = Some(Box::new(ActiveRust));
    let error = router
        .open_project(&fixture.0, &ProjectOpenOptions::default())
        .unwrap_err();
    assert!(error.contains("cannot start clangd"), "{error}");
    assert_eq!(router.project_options().language, ProjectLanguage::Rust);
    assert_eq!(router.files().unwrap(), [PathBuf::from("active.rs")]);
    let invalid = ProjectOpenOptions {
        language: ProjectLanguage::Rust,
        compilation_database: Some("bad".into()),
    };
    assert!(router.open_project(&fixture.0, &invalid).is_err());
    assert_eq!(router.files().unwrap(), [PathBuf::from("active.rs")]);
}

#[test]
fn unavailable_backends_leave_router_without_an_active_project() {
    let fixture = Fixture::new();
    fixture.write("main.c");
    let mut router = Opened::new(LanguageBackend::new(
        fixture.0.join("no-rust-analyzer"),
        fixture.0.join("no-clangd"),
    ));
    let error = router
        .open_project(&fixture.0, &ProjectOpenOptions::default())
        .unwrap_err();
    assert!(error.contains("REFSCAPE_CLANGD"), "{error}");
    assert_eq!(router.project_options(), ProjectOpenOptions::default());
    assert!(router.files().unwrap_err().contains("open a project"));
}

use refscape_analysis::{AnalysisSession, FeatureResult};

use std::time::Duration;
struct Opened<F> {
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
        self.session = Some(prepared.session);
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
impl Opened<LanguageBackend> {
    fn with_typescript_server(mut self, executable: impl Into<PathBuf>) -> Self {
        self.factory = self.factory.with_typescript_server(executable);
        self
    }
    fn with_pyright_server(mut self, executable: impl Into<PathBuf>) -> Self {
        self.factory = self.factory.with_pyright_server(executable);
        self
    }
}
