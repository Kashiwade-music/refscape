//! Real clangd coverage is opt-in because LLVM is an external dependency.
use refscape_application::ports::LanguageService;
use refscape_language_cpp::Clangd;
use refscape_model::{Position, ProjectLanguage, ProjectOptions, SourceRange, Symbol};
use serde_json::json;
use std::{
    env, fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "refscape-clangd-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn write(&self, path: &str, text: &str) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
    fn database(&self, paths: &[PathBuf], compiler: &str, flags: &[&str]) -> PathBuf {
        let entries: Vec<_> = paths
            .iter()
            .map(|path| {
                let mut args = vec![
                    compiler.to_string(),
                    "-I".into(),
                    self.0.join("source/include").display().to_string(),
                ];
                args.extend(flags.iter().map(|flag| flag.to_string()));
                args.extend(["-c".into(), path.display().to_string()]);
                json!({"directory":self.0.join("source"), "file":path, "arguments":args})
            })
            .collect();
        self.write(
            "build/debug/compile_commands.json",
            &json!(entries).to_string(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let temporary = env::temp_dir().canonicalize().unwrap();
        assert_eq!(self.0.parent(), Some(temporary.as_path()));
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("refscape-clangd-")
        );
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn at(text: &str, line: u32, word: &str) -> Position {
    let text = text.lines().nth(line as usize).unwrap();
    Position::new(
        line,
        text[..text.find(word).unwrap()].encode_utf16().count() as u32,
    )
}

fn eventually(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if check() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "clangd background index did not provide the expected result"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires clangd"]
fn cpp_method_and_function_cards_preserve_namespace_class_and_struct_context() {
    let fixture = Fixture::new();
    let text = "namespace outer {\n    namespace inner {\n        class Project {\n        public:\n            int first() const { return 1; }\n\n            int options() const {\n                return 42;\n            }\n        };\n        struct Settings {\n            int value() const { return 7; }\n        };\n\n        int helper() { return 3; }\n    }\n}\nint main() {\n    outer::inner::Project project;\n    int earlier = project.first();\n    return project.options() + earlier;\n}\n";
    let path = fixture.write("source/main.cpp", text);
    let database = fixture.database(std::slice::from_ref(&path), "clang++", &["-std=c++17"]);
    let mut language = Clangd::default().with_timeout(Duration::from_secs(30));
    language
        .open_project(
            &fixture.0.join("source"),
            &ProjectOptions {
                language: ProjectLanguage::Cpp,
                compilation_database: Some(database),
            },
        )
        .unwrap();
    fn find<'a>(symbols: &'a [Symbol], name: &str) -> Option<&'a Symbol> {
        symbols.iter().find_map(|symbol| {
            if symbol.name == name {
                Some(symbol)
            } else {
                find(&symbol.children, name)
            }
        })
    }
    let symbols = language.symbols(&path).unwrap();
    for (name, declaration_line, body_line) in [
        ("options", Some(2), 6),
        ("value", Some(10), 11),
        ("helper", None, 14),
    ] {
        let symbol = find(&symbols, name).unwrap_or_else(|| panic!("missing {name}: {symbols:?}"));
        let source = language.source(symbol).unwrap();
        let expected: Vec<_> = [Some(0), Some(1), declaration_line]
            .into_iter()
            .flatten()
            .map(|line| refscape_model::SourceContext {
                start_line: line,
                code: text.lines().nth(line as usize).unwrap().into(),
            })
            .collect();
        assert_eq!(source.context, expected, "{name}");
        assert_eq!(
            source.code_start,
            Some(Position::new(body_line, 0)),
            "{name}"
        );
        assert_eq!(
            source.code.lines().next(),
            text.lines().nth(body_line as usize),
            "{name}"
        );
        let row = source.display_row(Position::new(body_line, 16)).unwrap();
        assert_eq!(
            source.display_lines()[row].position,
            Some(Position::new(body_line, 0))
        );
        source.validate().unwrap();
    }
    let definitions = language
        .definitions(&path, at(text, 20, "options"))
        .unwrap();
    assert!(
        definitions.iter().any(|symbol| symbol.name == "options"),
        "{definitions:?}"
    );
    let source = language
        .source(
            definitions
                .iter()
                .find(|symbol| symbol.name == "options")
                .unwrap(),
        )
        .unwrap();
    assert_eq!(source.context.len(), 3);
    let main = language.source(find(&symbols, "main").unwrap()).unwrap();
    assert!(main.context.is_empty());

    // Actual C++ expansions use the same source-order placement as every other language.
    struct NoSession;
    impl refscape_application::ports::SessionRepository for NoSession {
        fn save(&self, _: &std::path::Path, _: &refscape_model::Session) -> Result<(), String> {
            unreachable!()
        }
        fn load(&self, _: &std::path::Path) -> Result<refscape_model::Session, String> {
            unreachable!()
        }
    }
    let options = language.project_options();
    let mut explorer = refscape_application::explorer::Explorer::new(language, NoSession);
    explorer
        .open_project(&fixture.0.join("source"), &options)
        .unwrap();
    let root = explorer
        .add_symbol(
            find(&symbols, "main").unwrap().clone(),
            refscape_model::Point::default(),
        )
        .unwrap();
    let later = explorer
        .expand_definition(&root, at(text, 20, "options"))
        .unwrap()
        .remove(0);
    let earlier = explorer
        .expand_definition(&root, at(text, 19, "first"))
        .unwrap()
        .remove(0);
    let cards = &explorer.session().cards;
    let later = cards.iter().find(|card| card.id == later).unwrap();
    let earlier = cards.iter().find(|card| card.id == earlier).unwrap();
    assert_eq!(earlier.position.x, later.position.x);
    assert!(earlier.position.y + earlier.display_height() < later.position.y);
    assert_eq!(earlier.source.context.len(), 3);
    assert_eq!(later.source.context.len(), 3);
    let later_id = later.id.clone();
    let target = explorer
        .expand_definition(&later_id, at(text, 2, "Project"))
        .unwrap()
        .remove(0);
    assert_eq!(
        explorer
            .session()
            .cards
            .iter()
            .find(|card| card.id == target)
            .unwrap()
            .source
            .symbol
            .name,
        "Project"
    );
    explorer.expand_context(&later_id, 2).unwrap();
    let card = explorer
        .session()
        .cards
        .iter()
        .find(|card| card.id == later_id)
        .unwrap();
    assert!(card.source.context[2].code.contains("int first()"));
    assert!(card.source.contains_display_position(at(text, 4, "first")));
    assert_eq!(card.source.display_row(at(text, 6, "options")), Some(6));
    let expanded = card.source.clone();
    explorer.collapse_context(&later_id, 2).unwrap();
    let folded = &explorer
        .session()
        .cards
        .iter()
        .find(|card| card.id == later_id)
        .unwrap()
        .source;
    assert_eq!(folded.context[2].code, "        class Project {");
    assert!(!folded.contains_display_position(at(text, 4, "first")));
    assert!(folded.expanded.is_empty());
    explorer.expand_context(&later_id, 2).unwrap();
    assert_eq!(
        explorer
            .session()
            .cards
            .iter()
            .find(|card| card.id == later_id)
            .unwrap()
            .source,
        expanded
    );
    explorer.session().validate().unwrap();
}

#[test]
#[ignore = "requires clangd; run cargo test -p refscape-language-cpp --test clangd -- --ignored"]
fn cpp_headers_navigation_tokens_and_failed_project_switches() {
    let fixture = Fixture::new();
    let header = fixture.write(
        "source/include/config.h",
        "#pragma once\nstruct Config { int count; };\nint answer(Config value);\n",
    );
    let implementation = fixture.write(
        "source/answer.cpp",
        "#include \"config.h\"\nint answer(Config value) { return value.count; }\n",
    );
    let text = "#include \"config.h\"\nint entry() {\n    Config value{42};\n    return answer(value);\n}\n";
    let main = fixture.write("source/main.cpp", text);
    let database = fixture.database(
        &[main.clone(), implementation.clone()],
        "clang++",
        &["-std=c++17"],
    );
    let root = fixture.0.join("source");
    let options = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(database.clone()),
    };
    let mut language = Clangd::default().with_timeout(Duration::from_secs(30));
    language.open_project(&root, &options).unwrap();
    assert_eq!(language.project_options(), options);
    assert!(language.project_crates().unwrap().is_empty());
    assert_eq!(
        language.files().unwrap(),
        [implementation.clone(), header.clone(), main.clone()]
    );
    // A fresh session must activate the database without a user opening a file.
    eventually(|| {
        language
            .search("entry")
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "entry")
    });
    let symbols = language.symbols(&main).unwrap();
    let entry = symbols
        .iter()
        .find(|symbol| symbol.name == "entry")
        .expect("entry symbol");
    assert!(
        language
            .source(entry)
            .unwrap()
            .code
            .starts_with("int entry")
    );
    let file = language
        .source(&Symbol::file(main.clone(), SourceRange::default()))
        .unwrap();
    assert_eq!(file.code, text);
    assert!(file.tokens.iter().any(|token| token.kind == "variable"));
    assert!(file.tokens.iter().any(|token| token.kind == "function"));
    let types = language
        .type_definitions(&main, at(text, 3, "value"))
        .unwrap();
    assert!(
        types
            .iter()
            .any(|symbol| symbol.path == header && symbol.name == "Config"),
        "{types:?}"
    );
    let highlights = language
        .document_highlights(&main, at(text, 3, "value"))
        .unwrap();
    assert!(
        highlights
            .iter()
            .any(|range| range.start == at(text, 2, "value"))
    );
    assert!(
        highlights
            .iter()
            .any(|range| range.start == at(text, 3, "value"))
    );
    assert!(
        language
            .hover(&main, at(text, 3, "value"))
            .unwrap()
            .unwrap()
            .contains("Config")
    );
    // Document requests wait for the per-file AST; cross-file results depend on the
    // asynchronous background index, so assert them with a bounded readiness wait.
    eventually(|| {
        language
            .definitions(&main, at(text, 3, "answer"))
            .unwrap()
            .iter()
            .any(|symbol| symbol.path == implementation)
    });
    eventually(|| {
        language
            .search("answer")
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "answer")
    });
    eventually(|| {
        language
            .references(&implementation, Position::new(1, 5))
            .unwrap()
            .iter()
            .any(|symbol| symbol.path == main && symbol.name == "entry")
    });
    let bad = ProjectOptions {
        compilation_database: Some(root.join("missing/compile_commands.json")),
        ..options.clone()
    };
    assert!(language.open_project(&root, &bad).is_err());
    assert_eq!(language.project_options(), options);
    assert!(
        language
            .symbols(&main)
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "entry")
    );
    let unknown = fixture.write("other/README.txt", "not a source project");
    assert!(
        language
            .open_project(
                unknown.parent().unwrap(),
                &refscape_model::ProjectOptions::default()
            )
            .is_err()
    );
    assert_eq!(language.project_options(), options);
    drop(language);
}

#[test]
#[ignore = "requires clangd; run cargo test -p refscape-language-cpp --test clangd -- --ignored"]
fn c_language_database_macros_and_no_database_fallback() {
    let fixture = Fixture::new();
    let header = fixture.write(
        "source/include/value.h",
        "#pragma once\nstruct Value { int count; };\n",
    );
    let text = "#include \"value.h\"\nint count(struct Value value) {\n    return value.count + REFSCAPE_FACTOR;\n}\n";
    let source = fixture.write("source/value.c", text);
    let database = fixture.database(
        std::slice::from_ref(&source),
        "clang",
        &["-std=c11", "-DREFSCAPE_FACTOR=7"],
    );
    let root = fixture.0.join("source");
    let mut language = Clangd::default().with_timeout(Duration::from_secs(30));
    language
        .open_project(
            &root,
            &ProjectOptions {
                language: ProjectLanguage::Cpp,
                compilation_database: Some(database),
            },
        )
        .unwrap();
    assert!(
        language
            .symbols(&source)
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "count")
    );
    assert!(
        language
            .type_definitions(&source, at(text, 2, "value"))
            .unwrap()
            .iter()
            .any(|symbol| symbol.path == header)
    );
    let hover = language
        .hover(&source, at(text, 2, "REFSCAPE_FACTOR"))
        .unwrap()
        .unwrap();
    assert!(hover.contains('7'), "{hover}");
    assert!(language.project_crates().unwrap().is_empty());
    let fallback = fixture.write("fallback/main.c", "int entry(void) { return 42; }\n");
    language
        .open_project(
            fallback.parent().unwrap(),
            &refscape_model::ProjectOptions::default(),
        )
        .unwrap();
    assert_eq!(language.project_options().compilation_database, None);
    assert!(
        language
            .symbols(&fallback)
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "entry")
    );
    let fallback_cpp = fixture.write(
        "fallback-cpp/main.cpp",
        "#include \"widget.h\"\nint entry() { Widget value{}; return value.count; }\n",
    );
    let fallback_header = fixture.write(
        "fallback-cpp/widget.h",
        "class Widget { public: int count = 42; };\n",
    );
    language
        .open_project(
            fallback_cpp.parent().unwrap(),
            &refscape_model::ProjectOptions::default(),
        )
        .unwrap();
    assert!(
        language
            .symbols(&fallback_header)
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "Widget")
    );
    assert!(
        language
            .search("Widget")
            .unwrap()
            .iter()
            .any(|symbol| symbol.path == fallback_header)
    );
    drop(language);
}

#[test]
fn startup_errors_identify_the_required_c_cpp_server() {
    let fixture = Fixture::new();
    fixture.write("main.cpp", "int main() {}");
    let mut language = Clangd::new(fixture.0.join("no-clangd"));
    let error = language
        .open_project(&fixture.0, &refscape_model::ProjectOptions::default())
        .unwrap_err();
    assert!(
        error.contains("cannot start clangd") && error.contains("REFSCAPE_CLANGD"),
        "{error}"
    );
    assert!(!error.contains("rustup"), "{error}");
    assert_eq!(language.project_options(), ProjectOptions::default());
}
