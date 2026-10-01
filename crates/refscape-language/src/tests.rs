use super::*;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "refscape-router-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
            Some(env::temp_dir().canonicalize().unwrap().as_path())
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
    assert!(select_language(&fixture.0, &ProjectOptions::default()).is_err());
    fixture.write("main.cpp");
    assert_eq!(
        select_language(&fixture.0, &ProjectOptions::default()).unwrap(),
        ProjectLanguage::Cpp
    );
    fixture.write("Cargo.toml");
    assert_eq!(
        select_language(&fixture.0, &ProjectOptions::default()).unwrap(),
        ProjectLanguage::Rust
    );
    let database = ProjectOptions {
        compilation_database: Some("build".into()),
        ..ProjectOptions::default()
    };
    assert_eq!(
        select_language(&fixture.0, &database).unwrap(),
        ProjectLanguage::Cpp
    );
    let cpp = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: None,
    };
    assert_eq!(
        select_language(&fixture.0, &cpp).unwrap(),
        ProjectLanguage::Cpp
    );
    let invalid = ProjectOptions {
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
        select_language(&fixture.0, &ProjectOptions::default()).unwrap(),
        ProjectLanguage::TypeScript
    );
    let explicit = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: None,
    };
    assert_eq!(
        select_language(&fixture.0, &explicit).unwrap(),
        ProjectLanguage::Cpp
    );
    let database = ProjectOptions {
        compilation_database: Some("build".into()),
        ..ProjectOptions::default()
    };
    assert_eq!(
        select_language(&fixture.0, &database).unwrap(),
        ProjectLanguage::Cpp
    );
    let invalid = ProjectOptions {
        language: ProjectLanguage::TypeScript,
        ..database
    };
    assert!(select_language(&fixture.0, &invalid).is_err());
    fixture.write("Cargo.toml");
    assert_eq!(
        select_language(&fixture.0, &ProjectOptions::default()).unwrap(),
        ProjectLanguage::Rust
    );
}

#[test]
fn failed_typescript_start_preserves_previous_project() {
    let fixture = Fixture::new();
    fixture.write("App.jsx");
    let mut router =
        LanguageBackend::default().with_typescript_server(fixture.0.join("no-typescript-server"));
    router.active = Some(Box::new(ActiveRust));
    let error = router
        .open_project(&fixture.0, &ProjectOptions::default())
        .unwrap_err();
    assert!(
        error.contains("REFSCAPE_TYPESCRIPT_LANGUAGE_SERVER"),
        "{error}"
    );
    assert_eq!(router.project_options().language, ProjectLanguage::Rust);
    assert_eq!(router.files().unwrap(), [PathBuf::from("active.rs")]);
}

struct ActiveRust;
impl LanguageService for ActiveRust {
    fn open_project(&mut self, _: &Path, _: &ProjectOptions) -> Result<(), String> {
        Ok(())
    }
    fn project_options(&self) -> ProjectOptions {
        ProjectOptions {
            language: ProjectLanguage::Rust,
            compilation_database: None,
        }
    }
    fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        Ok(vec!["active.rs".into()])
    }
    fn symbols(&mut self, _: &Path) -> Result<Vec<Symbol>, String> {
        Ok(vec![])
    }
    fn source(&mut self, _: &Symbol) -> Result<SourceDocument, String> {
        Err("not needed".into())
    }
    fn definitions(&mut self, _: &Path, _: Position) -> Result<Vec<Symbol>, String> {
        Ok(vec![])
    }
    fn references(&mut self, _: &Path, _: Position) -> Result<Vec<Symbol>, String> {
        Ok(vec![])
    }
}

#[test]
fn failed_cross_language_start_preserves_active_analysis_and_options() {
    let fixture = Fixture::new();
    fixture.write("main.cpp");
    let mut router = LanguageBackend::new(
        fixture.0.join("no-rust-analyzer"),
        fixture.0.join("no-clangd"),
    );
    router.active = Some(Box::new(ActiveRust));
    let error = router
        .open_project(&fixture.0, &ProjectOptions::default())
        .unwrap_err();
    assert!(error.contains("cannot start clangd"), "{error}");
    assert_eq!(router.project_options().language, ProjectLanguage::Rust);
    assert_eq!(router.files().unwrap(), [PathBuf::from("active.rs")]);
    let invalid = ProjectOptions {
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
    let mut router = LanguageBackend::new(
        fixture.0.join("no-rust-analyzer"),
        fixture.0.join("no-clangd"),
    );
    let error = router
        .open_project(&fixture.0, &ProjectOptions::default())
        .unwrap_err();
    assert!(error.contains("REFSCAPE_CLANGD"), "{error}");
    assert_eq!(router.project_options(), ProjectOptions::default());
    assert!(router.files().unwrap_err().contains("open a project"));
}
