//! Real-server coverage, opt-in because rust-analyzer is an external toolchain component.
mod support;
use refscape_language_rust::RustAnalyzer;
use refscape_model::{Position, SourceRange, Symbol};
use std::{
    env, fs,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[test]
#[ignore = "requires rust-analyzer"]
fn real_server_preserves_nested_module_and_impl_context_on_method_cards() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = env::temp_dir().join(format!("refscape-context-{}-{unique}", std::process::id()));
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"context_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n").unwrap();
    let code = "mod outer {\n    pub struct CppProject;\n    impl CppProject {\n        pub fn first(&self) {}\n\n        pub(crate) fn options(&self) -> u32 {\n            42\n        }\n    }\n}\nfn main() {}\n";
    let path = root.join("src/main.rs");
    fs::write(&path, code).unwrap();
    let mut language =
        support::Opened::new(RustAnalyzer::default()).with_timeout(Duration::from_secs(30));
    language
        .open_project(&root, &refscape_model::ProjectOpenOptions::default())
        .unwrap();
    fn find(symbols: &[Symbol]) -> Option<&Symbol> {
        symbols.iter().find_map(|symbol| {
            if symbol.name == "options" {
                Some(symbol)
            } else {
                find(&symbol.children)
            }
        })
    }
    let symbols = language.symbols(&path).unwrap();
    let method = find(&symbols).expect("options method from rust-analyzer");
    let source = language.source(method).unwrap();
    assert_eq!(
        source.context,
        vec![
            refscape_model::SourceContext {
                start_line: 0,
                code: "mod outer {".into()
            },
            refscape_model::SourceContext {
                start_line: 2,
                code: "    impl CppProject {".into()
            },
        ]
    );
    assert!(
        source.code.starts_with("        pub(crate) fn options"),
        "{}",
        source.code
    );
    assert_eq!(source.code_start, Some(Position::new(5, 0)));
    let projection = refscape_model::CardSource::try_from(source.clone()).unwrap();
    let rows = projection.display_lines();
    assert_eq!(
        rows.iter()
            .map(|row| row.position.map(|p| p.line))
            .collect::<Vec<_>>(),
        vec![Some(0), None, Some(2), None, Some(5), Some(6), Some(7)]
    );
    assert_eq!(projection.display_row(Position::new(5, 30)), Some(4));
    source.validate().unwrap();
    assert!(source.folded[1].code.contains("pub fn first"));
    let type_column = code.lines().nth(2).unwrap().find("CppProject").unwrap() as u32;
    let targets = language
        .definitions(&path, Position::new(2, type_column))
        .unwrap();
    assert_eq!(targets[0].kind, "struct");
    let mut card = refscape_model::CardSource::try_from(source.clone()).unwrap();
    card.toggle_fold(1).unwrap();
    assert_eq!(card.display_row(Position::new(5, 30)), Some(5));
    assert!(card.contains_display_position(Position::new(3, 15)));
    assert!(card.export_context()[1].code.contains("pub fn first"));
    let expanded = card.to_document();
    card.toggle_fold(1).unwrap();
    assert_eq!(card.to_document(), source);
    card.toggle_fold(1).unwrap();
    assert_eq!(card.to_document(), expanded);
    card.validate().unwrap();
    drop(language);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires rust-analyzer; run cargo test -p refscape-language-rust --test rust_analyzer -- --ignored"]
fn real_server_resolves_inferred_variable_types_and_scope_aware_highlights() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = env::temp_dir().join(format!("refscape-variable-{}-{unique}", std::process::id()));
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"refscape_variable_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n").unwrap();
    let code = "mod config;\nuse config::Config;\npub fn entry(config: Config) -> u32 {\n    let value = config;\n    let _first = &value;\n    {\n        let value = 7u32;\n        let _second = value;\n    }\n    let _last = &value;\n    42\n}\n";
    let lib = root.join("src/lib.rs");
    fs::write(&lib, code).unwrap();
    fs::write(
        root.join("src/config.rs"),
        "pub struct Config {\n    pub count: u32,\n}\n",
    )
    .unwrap();
    let at = |line: u32, word: &str| {
        let text = code.lines().nth(line as usize).unwrap();
        let byte = text.find(word).unwrap();
        Position::new(line, text[..byte].encode_utf16().count() as u32)
    };
    let range = |line: u32, word: &str| {
        let start = at(line, word);
        SourceRange {
            start,
            end: Position::new(line, start.character + word.encode_utf16().count() as u32),
        }
    };
    let mut language =
        support::Opened::new(RustAnalyzer::default()).with_timeout(Duration::from_secs(30));
    language
        .open_project(&root, &refscape_model::ProjectOpenOptions::default())
        .unwrap();
    let file = language
        .source(&Symbol::file(lib.clone(), SourceRange::default()))
        .unwrap();
    let file_projection = refscape_model::CardSource::try_from(file.clone()).unwrap();
    assert!(file_projection.variable_token(at(4, "value")).is_some());
    assert!(file_projection.variable_token(at(2, "config")).is_some());
    assert!(file_projection.variable_token(at(2, "entry")).is_none());
    let types = language.type_definitions(&lib, at(4, "value")).unwrap();
    assert_eq!(types.len(), 1);
    assert_eq!(types[0].name, "Config");
    assert_eq!(
        types[0].path,
        root.join("src/config.rs").canonicalize().unwrap()
    );
    assert!(
        language
            .source(&types[0])
            .unwrap()
            .code
            .contains("pub count: u32")
    );
    let highlights = language.document_highlights(&lib, at(4, "value")).unwrap();
    assert!(highlights.contains(&range(3, "value")), "{highlights:?}");
    assert!(highlights.contains(&range(4, "value")), "{highlights:?}");
    assert!(highlights.contains(&range(9, "value")), "{highlights:?}");
    assert!(
        !highlights
            .iter()
            .any(|range| matches!(range.start.line, 6 | 7)),
        "{highlights:?}"
    );
    let hover = language.hover(&lib, at(4, "value")).unwrap().unwrap();
    assert!(hover.contains("Config"), "{hover}");
    assert!(
        language
            .type_definitions(&lib, at(7, "value"))
            .unwrap()
            .is_empty()
    );
    let primitive = language.document_highlights(&lib, at(7, "value")).unwrap();
    assert!(primitive.contains(&range(6, "value")));
    assert!(primitive.contains(&range(7, "value")));
    assert!(
        !primitive
            .iter()
            .any(|range| matches!(range.start.line, 3 | 4 | 9))
    );
    let parameter_type = language.type_definitions(&lib, at(3, "config")).unwrap();
    assert_eq!(parameter_type, types);
    let binding = language.definitions(&lib, at(4, "value")).unwrap();
    assert!(
        binding
            .iter()
            .all(|symbol| symbol.path == lib.canonicalize().unwrap())
    );
    drop(language);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires rust-analyzer; run cargo test -p refscape-language-rust --test rust_analyzer -- --ignored"]
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
        "/// Returns the answer.\npub fn answer() -> u32 {\n    42\n}\n",
    )
    .unwrap();
    let mut language =
        support::Opened::new(RustAnalyzer::default()).with_timeout(Duration::from_secs(20));
    language
        .open_project(&root, &refscape_model::ProjectOpenOptions::default())
        .unwrap();
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
    let hover = language
        .hover(&lib, Position::new(2, 13))
        .unwrap()
        .expect("hover from rust-analyzer");
    assert!(hover.contains("fn answer() -> u32"), "{hover}");
    assert!(hover.contains("Returns the answer."), "{hover}");
    assert!(
        !hover.contains("```"),
        "hover must use negotiated plain text: {hover}"
    );
    let references = language
        .references(
            &helper,
            Position {
                line: 1,
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
    assert!(
        language
            .open_project(&broken, &refscape_model::ProjectOpenOptions::default())
            .is_err()
    );
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
#[ignore = "requires rust-analyzer; run cargo test -p refscape-language-rust --test rust_analyzer -- --ignored"]
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
    let mut language =
        support::Opened::new(RustAnalyzer::default()).with_timeout(Duration::from_secs(60));
    language
        .open_project(&root, &refscape_model::ProjectOpenOptions::default())
        .unwrap();
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
