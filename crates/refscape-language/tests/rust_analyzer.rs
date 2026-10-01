//! Real-server coverage, opt-in because rust-analyzer is an external toolchain component.
use refscape_application::LanguageService;
use refscape_language::RustAnalyzer;
use refscape_model::{Position, SourceRange, Symbol};
use std::{
    env, fs,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[test]
#[ignore = "requires rust-analyzer; run cargo test -p refscape-language --test rust_analyzer -- --ignored"]
fn real_server_follows_cross_file_definitions_references_and_highlights() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = env::temp_dir().join(format!("refscape-lsp-{}-{unique}", std::process::id()));
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("Cargo.toml"),"[package]\nname = \"refscape_lsp_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n").unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "mod helper;\npub fn entry() -> u32 {\n    helper::answer()\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/helper.rs"),
        "pub fn answer() -> u32 {\n    42\n}\n",
    )
    .unwrap();
    let mut language = RustAnalyzer::default().with_timeout(Duration::from_secs(20));
    language.open_project(&root).unwrap();
    let crates = language.project_crates().unwrap();
    assert_eq!(crates.len(), 1);
    assert_eq!(crates[0].name, "refscape_lsp_fixture");
    assert_eq!(crates[0].root, root.canonicalize().unwrap());
    let lib = root.join("src/lib.rs");
    let helper = root.join("src/helper.rs");
    assert_eq!(language.files().unwrap().len(), 2);
    let symbols = language.symbols(&lib).unwrap();
    let entry = symbols
        .iter()
        .find(|symbol| symbol.name == "entry")
        .expect("entry symbol from rust-analyzer");
    let source = language.source(entry).unwrap();
    assert!(source.code.starts_with("pub fn entry"));
    assert!(source.code.ends_with('}'));
    assert!(source.tokens.iter().any(|token| token.kind == "function"));
    let definitions = language
        .definitions(
            &lib,
            Position {
                line: 2,
                character: 13,
            },
        )
        .unwrap();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].name, "answer");
    assert_eq!(definitions[0].path, helper.canonicalize().unwrap());
    let answer_source = language.source(&definitions[0]).unwrap();
    assert!(answer_source.code.contains("42"));
    let references = language
        .references(
            &helper,
            Position {
                line: 0,
                character: 8,
            },
        )
        .unwrap();
    assert!(references.iter().any(|symbol| symbol.name == "entry"));
    assert!(
        language
            .search("answer")
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "answer")
    );
    let file = language
        .source(&Symbol::file(lib.clone(), SourceRange::default()))
        .unwrap();
    assert_eq!(file.code, fs::read_to_string(&lib).unwrap());
    // A failed switch must leave the old rust-analyzer process, metadata and source usable.
    let broken = root.join("broken-project");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("Cargo.toml"), "[package]\nname =\n").unwrap();
    assert!(language.open_project(&broken).is_err());
    assert_eq!(language.project_crates().unwrap(), crates);
    assert!(
        language
            .symbols(&lib)
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "entry")
    );
    assert_eq!(language.source(entry).unwrap().code, source.code);
    assert!(
        language
            .definitions(
                &lib,
                Position {
                    line: 2,
                    character: 13
                }
            )
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "answer")
    );
    // External source modifications invalidate document symbols and token caches.
    fs::write(
        &lib,
        "mod helper;\npub fn changed() -> u32 { helper::answer() }\n",
    )
    .unwrap();
    assert!(
        language
            .symbols(&lib)
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "changed")
    );
    drop(language);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires rust-analyzer; run cargo test -p refscape-language --test rust_analyzer -- --ignored"]
fn real_workspace_packages_use_cargo_boundaries_and_custom_targets() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = env::temp_dir().join(format!(
        "refscape-workspace-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("alpha/custom")).unwrap();
    fs::create_dir_all(root.join("beta/src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"alpha\", \"beta\"]\nresolver = \"3\"\n",
    )
    .unwrap();
    fs::write(root.join("alpha/Cargo.toml"),"[package]\nname = \"alpha-package\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\npath = \"custom/entry.rs\"\n").unwrap();
    fs::write(
        root.join("beta/Cargo.toml"),
        "[package]\nname = \"beta-package\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(root.join("alpha/custom/entry.rs"), "pub fn alpha() {}\n").unwrap();
    fs::write(root.join("beta/src/lib.rs"), "pub fn beta() {}\n").unwrap();
    fs::write(
        root.join("unrelated.rs"),
        "this is outside the Cargo workspace packages\n",
    )
    .unwrap();
    let mut language = RustAnalyzer::default().with_timeout(Duration::from_secs(60));
    language.open_project(&root).unwrap();
    let crates = language.project_crates().unwrap();
    assert_eq!(
        crates
            .iter()
            .map(|package| package.name.as_str())
            .collect::<Vec<_>>(),
        ["alpha-package", "beta-package"]
    );
    assert_eq!(crates[0].root, root.join("alpha").canonicalize().unwrap());
    assert_eq!(crates[1].root, root.join("beta").canonicalize().unwrap());
    assert_ne!(crates[0].id, crates[1].id);
    let files = language.files().unwrap();
    assert_eq!(files.len(), 2);
    assert!(files.contains(&root.join("alpha/custom/entry.rs").canonicalize().unwrap()));
    assert!(
        language
            .search("alpha")
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "alpha")
    );
    drop(language);
    fs::remove_dir_all(root).unwrap();
}
