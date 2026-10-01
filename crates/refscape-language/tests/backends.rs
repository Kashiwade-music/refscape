//! End-to-end routing across independent language adapters.
use refscape_application::ports::LanguageService;
use refscape_language::LanguageBackend;
use refscape_model::{ProjectLanguage, ProjectOptions};
use std::{
    env, fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = env::temp_dir().join(format!(
            "refscape-backends-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("rust/src")).unwrap();
        fs::create_dir_all(root.join("cpp")).unwrap();
        fs::write(root.join("rust/Cargo.toml"), "[package]\nname = \"routing_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n").unwrap();
        fs::write(
            root.join("rust/src/lib.rs"),
            "pub fn rust_answer() -> u32 { 42 }\n",
        )
        .unwrap();
        fs::write(
            root.join("cpp/main.cpp"),
            "int cpp_answer() { return 42; }\n",
        )
        .unwrap();
        Self(root.canonicalize().unwrap())
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
                .starts_with("refscape-backends-")
        );
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires rust-analyzer and clangd; run cargo test -p refscape-language --test backends -- --ignored"]
fn router_switches_rust_cpp_and_back_and_retains_analysis_after_failed_switch() {
    let fixture = Fixture::new();
    let rust = fixture.0.join("rust");
    let cpp = fixture.0.join("cpp");
    let mut backend = LanguageBackend::default().with_timeout(Duration::from_secs(30));
    backend
        .open_project(&rust, &ProjectOptions::default())
        .unwrap();
    assert_eq!(backend.project_options().language, ProjectLanguage::Rust);
    assert_eq!(backend.project_crates().unwrap()[0].name, "routing_fixture");
    assert!(
        backend
            .symbols(&rust.join("src/lib.rs"))
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "rust_answer")
    );

    backend
        .open_project(&cpp, &ProjectOptions::default())
        .unwrap();
    assert_eq!(backend.project_options().language, ProjectLanguage::Cpp);
    assert!(backend.project_crates().unwrap().is_empty());
    assert!(
        backend
            .symbols(&cpp.join("main.cpp"))
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "cpp_answer")
    );
    let invalid = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some("missing/compile_commands.json".into()),
    };
    assert!(backend.open_project(&cpp, &invalid).is_err());
    assert_eq!(backend.project_options().language, ProjectLanguage::Cpp);
    assert!(
        backend
            .search("cpp_answer")
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "cpp_answer")
    );

    backend
        .open_project(&rust, &ProjectOptions::default())
        .unwrap();
    assert_eq!(backend.project_options().language, ProjectLanguage::Rust);
    assert!(
        backend
            .symbols(&rust.join("src/lib.rs"))
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "rust_answer")
    );
    drop(backend);
}

#[test]
#[ignore = "requires rust-analyzer, clangd, Node.js and npm install in examples/typescript-demo"]
fn router_switches_all_three_languages_and_preserves_typescript_on_failed_switch() {
    let fixture = Fixture::new();
    let typescript = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/typescript-demo")
        .canonicalize()
        .unwrap();
    let mut backend = LanguageBackend::default().with_timeout(Duration::from_secs(45));
    for (root, language) in [
        (fixture.0.join("rust"), ProjectLanguage::Rust),
        (fixture.0.join("cpp"), ProjectLanguage::Cpp),
        (typescript.clone(), ProjectLanguage::TypeScript),
    ] {
        backend
            .open_project(&root, &ProjectOptions::default())
            .unwrap();
        assert_eq!(backend.project_options().language, language);
        assert!(!backend.files().unwrap().is_empty());
    }
    let invalid = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some("missing/compile_commands.json".into()),
    };
    assert!(
        backend
            .open_project(&fixture.0.join("cpp"), &invalid)
            .is_err()
    );
    assert_eq!(
        backend.project_options().language,
        ProjectLanguage::TypeScript
    );
    assert!(!backend.search("CounterView").unwrap().is_empty());
    backend
        .open_project(&fixture.0.join("rust"), &ProjectOptions::default())
        .unwrap();
    assert_eq!(backend.project_options().language, ProjectLanguage::Rust);
}
