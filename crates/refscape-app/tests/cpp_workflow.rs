//! Complete C/C++ adapter composition using a real clangd; opt-in like workflow.rs.

use std::{
    env, fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[path = "common/workflow.rs"]
mod common;
use common::Workflow;
use refscape_language::LanguageBackend;
use refscape_model::{
    ConnectionKind, Point, Position, ProjectLanguage, ProjectOpenOptions, Symbol,
};
use refscape_storage::session::JsonSessionRepository;

type TestExplorer = Workflow;

struct Fixture {
    temp: PathBuf,
    root: PathBuf,
    database: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp = env::temp_dir().join(format!(
            "refscape-cpp-workflow-{}-{unique}",
            std::process::id()
        ));
        let root = temp.join("source");
        let build = temp.join("external build");
        fs::create_dir_all(root.join("include")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(&build).unwrap();
        let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/cpp-demo");
        for relative in [
            "include/scale.h",
            "include/counter.hpp",
            "src/scale.c",
            "src/entry.cpp",
        ] {
            fs::copy(demo.join(relative), root.join(relative)).unwrap();
        }
        let temp = temp.canonicalize().unwrap();
        let root = root.canonicalize().unwrap();
        let build = build.canonicalize().unwrap();
        let database = build.join("compile_commands.json");
        let commands = [
            ("src/scale.c", "c", "c11"),
            ("src/entry.cpp", "c++", "c++17"),
        ]
        .map(|(relative, language, standard)| {
            let file = root.join(relative);
            let driver = compiler(language == "c++");
            let arguments = [
                json_path(&driver),
                json_string("-x"),
                json_string(language),
                json_string(&format!("-std={standard}")),
                json_string("-I"),
                json_path(&root.join("include")),
                json_string("-DREFSCAPE_DEMO_FACTOR=3"),
                json_string("-c"),
                json_path(&file),
            ];
            format!(
                "{{\"directory\":{},\"file\":{},\"arguments\":[{}]}}",
                json_path(&build),
                json_path(&file),
                arguments.join(",")
            )
        });
        fs::write(&database, format!("[{}]", commands.join(","))).unwrap();
        Self {
            temp,
            root,
            database: database.canonicalize().unwrap(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only recursively remove the uniquely named fixture directly in the temp directory.
        let temp = env::temp_dir().canonicalize().unwrap();
        if self.temp.parent() == Some(temp.as_path())
            && self
                .temp
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .starts_with("refscape-cpp-workflow-")
        {
            let _ = fs::remove_dir_all(&self.temp);
        }
    }
}

fn clangd() -> PathBuf {
    env::var_os("REFSCAPE_CLANGD")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let installed = PathBuf::from("C:/Program Files/LLVM/bin/clangd.exe");
            if cfg!(windows) && installed.is_file() {
                installed
            } else {
                "clangd".into()
            }
        })
}

fn compiler(cpp: bool) -> PathBuf {
    let name = match (cpp, cfg!(windows)) {
        (true, true) => "clang++.exe",
        (false, true) => "clang.exe",
        (true, false) => "clang++",
        (false, false) => "clang",
    };
    clangd()
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from(name), |parent| parent.join(name))
}

fn explorer() -> TestExplorer {
    Workflow::new(
        LanguageBackend::new("rust-analyzer", clangd()),
        JsonSessionRepository,
    )
}

fn arrange_and_undo(explorer: &mut TestExplorer, root: &str) {
    let ids: Vec<_> = explorer
        .session()
        .cards
        .iter()
        .filter(|card| card.id != root)
        .map(|card| card.id.clone())
        .collect();
    for (index, id) in ids.iter().enumerate() {
        explorer
            .move_card(
                id,
                Point::new(
                    10000.0 + index as f32 * 2000.0,
                    5000.0 + index as f32 * 500.0,
                ),
            )
            .unwrap();
    }
    let before = explorer.session().clone();
    assert!(explorer.arrange_layout(Some(root)).unwrap());
    assert_eq!(explorer.session().connections, before.connections);
    assert_eq!(explorer.session().viewport, before.viewport);
    for original in before.cards.iter() {
        let current = explorer
            .session()
            .cards
            .iter()
            .find(|card| card.id == original.id)
            .unwrap();
        assert_eq!(current.source, original.source);
        assert_eq!(
            (current.width, current.height),
            (original.width, original.height)
        );
        if original.id == root {
            assert_eq!(current.position, original.position);
        }
    }
    assert!(explorer.undo_layout().unwrap());
    assert_eq!(explorer.session().cards, before.cards);
    assert!(explorer.arrange_layout(Some(root)).unwrap());
}

fn json_path(path: &Path) -> String {
    // clang accepts ordinary slash-separated Windows paths; omit Rust's verbatim prefix.
    json_string(
        &path
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('\\', "/"),
    )
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn position(path: &Path, text: &str) -> Position {
    let source = fs::read_to_string(path).unwrap();
    let offset = source
        .find(text)
        .unwrap_or_else(|| panic!("{text:?} not in {}", path.display()));
    let preceding = &source[..offset];
    Position::new(
        preceding.bytes().filter(|byte| *byte == b'\n').count() as u32,
        preceding
            .rsplit('\n')
            .next()
            .unwrap()
            .encode_utf16()
            .count() as u32,
    )
}

fn named_symbol(explorer: &mut TestExplorer, path: &Path, name: &str) -> Symbol {
    explorer
        .symbols(path)
        .unwrap()
        .into_iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("clangd did not return {name:?} in {}", path.display()))
}

#[test]
#[ignore = "requires clangd; run cargo test -p refscape-app --test cpp_workflow -- --ignored"]
fn real_c_and_cpp_navigation_and_external_database_session_restore() {
    let fixture = Fixture::new();
    let options = ProjectOpenOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(fixture.database.clone()),
    };
    let mut explorer = explorer();
    explorer.open_project(&fixture.root, &options).unwrap();
    assert_eq!(explorer.session().project_root, fixture.root);
    assert_eq!(explorer.session().project_options, options);
    assert!(!fixture.database.starts_with(&fixture.root));
    let files = explorer.files().unwrap();
    assert_eq!(files.len(), 4);
    for relative in [
        "src/entry.cpp",
        "src/scale.c",
        "include/counter.hpp",
        "include/scale.h",
    ] {
        assert!(files.contains(&fixture.root.join(relative).canonicalize().unwrap()));
    }

    let entry_path = fixture.root.join("src/entry.cpp");
    let scale_path = fixture.root.join("src/scale.c");
    let header_path = fixture.root.join("include/counter.hpp");
    let entry_symbol = named_symbol(&mut explorer, &entry_path, "entry");
    let entry = explorer
        .add_symbol(entry_symbol, Point::new(30.0, 40.0))
        .unwrap();
    let scale_symbol = named_symbol(&mut explorer, &scale_path, "scale");
    let scale_name = scale_symbol.selection_range.start;
    let scale = explorer
        .add_symbol(scale_symbol, Point::new(30.0, 500.0))
        .unwrap();
    let scale_card = explorer
        .session()
        .cards
        .iter()
        .find(|card| card.id == scale)
        .unwrap();
    assert!(scale_card.source.code.contains("REFSCAPE_DEMO_FACTOR"));
    assert!(
        scale_card
            .source
            .tokens
            .iter()
            .any(|token| token.kind == "function")
    );
    let macro_hover = explorer
        .hover(&scale, position(&scale_path, "REFSCAPE_DEMO_FACTOR;"))
        .unwrap();
    assert!(
        macro_hover
            .unwrap()
            .contains("#define REFSCAPE_DEMO_FACTOR 3")
    );
    assert!(
        explorer
            .hover(&entry, position(&entry_path, "scale(2)"))
            .unwrap()
            .unwrap()
            .contains("scale")
    );

    // A background index supplies the C definition for the C++ call. Poll a bounded
    // interval because clangd can initially return only the declaration in scale.h.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let targets = explorer
            .expand_definition(&entry, position(&entry_path, "scale(2)"))
            .unwrap();
        if targets.contains(&scale) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "cross-language C definition was not indexed"
        );
        thread::sleep(Duration::from_millis(100));
    }
    loop {
        if explorer
            .expand_references(&scale, scale_name)
            .unwrap()
            .contains(&entry)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "C++ reference to C function was not indexed"
        );
        thread::sleep(Duration::from_millis(100));
    }
    assert!(explorer.session().connections.iter().any(|edge| {
        edge.kind == ConnectionKind::Reference && edge.from == scale && edge.to == entry
    }));
    let types = explorer
        .toggle_type_definition(&entry, position(&entry_path, "counter{7}"))
        .unwrap()
        .unwrap();
    assert!(
        types
            .iter()
            .any(|id| explorer.session().cards.iter().any(|card| {
                &card.id == id
                    && card.source.symbol.name == "Counter"
                    && card.source.symbol.path == header_path
            }))
    );
    assert!(
        explorer
            .session()
            .connections
            .iter()
            .any(|edge| { edge.kind == ConnectionKind::TypeDefinition && edge.from == entry })
    );
    let count = explorer.session().cards.len();
    let before_reuse = explorer.session().cards.clone();
    assert!(
        explorer
            .expand_definition(&entry, position(&entry_path, "scale(2)"))
            .unwrap()
            .contains(&scale)
    );
    assert_eq!(explorer.session().cards.len(), count);
    for original in before_reuse.iter() {
        assert_eq!(
            explorer
                .session()
                .cards
                .iter()
                .find(|card| card.id == original.id)
                .unwrap()
                .position,
            original.position
        );
    }
    explorer.pan(Point::new(150.0, -45.0)).unwrap();
    arrange_and_undo(&mut explorer, &entry);
    explorer.session().validate().unwrap();

    let session_path = fixture.temp.join("sessions/cpp-review.json");
    explorer.save_session(&session_path).unwrap();
    let saved = explorer.session().clone();
    drop(explorer);
    let mut restored = self::explorer();
    restored.load_session(&session_path).unwrap();
    assert_eq!(restored.session(), &saved);
    // Exercise the newly started backend, proving restore preserved the external
    // database rather than merely deserializing the saved cards.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if restored
            .search("entry")
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "entry")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "restored project's workspace symbols were not indexed"
        );
        thread::sleep(Duration::from_millis(100));
    }
    assert!(
        restored
            .hover(&scale, position(&scale_path, "REFSCAPE_DEMO_FACTOR;"))
            .unwrap()
            .unwrap()
            .contains("#define REFSCAPE_DEMO_FACTOR 3")
    );
    assert!(
        restored
            .expand_definition(&entry, position(&entry_path, "scale(2)"))
            .unwrap()
            .contains(&scale)
    );
    drop(restored);
}

#[test]
#[ignore = "requires clangd; run cargo test -p refscape-app --test cpp_workflow -- --ignored"]
fn simple_c_project_opens_without_a_compilation_database() {
    let fixture = Fixture::new();
    // Replace the demo with standalone C + header; the external database remains
    // outside the source tree and is intentionally not selected.
    for relative in [
        "src/entry.cpp",
        "src/scale.c",
        "include/counter.hpp",
        "include/scale.h",
    ] {
        fs::remove_file(fixture.root.join(relative)).unwrap();
    }
    let header = fixture.root.join("helper.h");
    let source = fixture.root.join("main.c");
    fs::write(
        &header,
        "#pragma once\nstatic inline int answer(void) { return 42; }\n",
    )
    .unwrap();
    fs::write(
        &source,
        "#include \"helper.h\"\nint entry(void) { return answer(); }\n",
    )
    .unwrap();
    let mut explorer = explorer();
    explorer
        .open_project(&fixture.root, &ProjectOpenOptions::default())
        .unwrap();
    assert_eq!(
        explorer.session().project_options.language,
        ProjectLanguage::Cpp
    );
    assert_eq!(
        explorer.session().project_options.compilation_database,
        None
    );
    assert_eq!(explorer.files().unwrap().len(), 2);
    let symbol = named_symbol(&mut explorer, &source, "entry");
    let entry = explorer.add_symbol(symbol, Point::default()).unwrap();
    let call = position(&source, "answer()");
    let targets = explorer.expand_definition(&entry, call).unwrap();
    assert!(
        targets
            .iter()
            .any(|id| explorer.session().cards.iter().any(|card| {
                &card.id == id
                    && card.source.symbol.path == header
                    && card.source.code.contains("42")
            }))
    );
    assert!(
        explorer
            .hover(&entry, call)
            .unwrap()
            .unwrap()
            .contains("answer")
    );
    assert!(
        explorer
            .session()
            .cards
            .iter()
            .any(|card| !card.source.tokens.is_empty())
    );
    arrange_and_undo(&mut explorer, &entry);
    explorer.session().validate().unwrap();
    drop(explorer);
}

#[test]
#[ignore = "requires clangd"]
fn real_cpp_navigation_preserves_source_order_placement_and_fold_projection() {
    let fixture = Fixture::new();
    let text = "namespace outer {\n    namespace inner {\n        class Project {\n        public:\n            int first() const { return 1; }\n\n            int options() const {\n                return 42;\n            }\n        };\n        struct Settings {\n            int value() const { return 7; }\n        };\n\n        int helper() { return 3; }\n    }\n}\nint main() {\n    outer::inner::Project project;\n    int earlier = project.first();\n    return project.options() + earlier;\n}\n";
    let path = fixture.root.join("main.cpp");
    fs::write(&path, text).unwrap();
    fs::write(
        fixture.root.join(".clangd"),
        "CompileFlags:\n  Add: [-std=c++17]\n",
    )
    .unwrap();
    let mut workflow = explorer();
    workflow
        .open_project(
            &fixture.root,
            &ProjectOpenOptions {
                language: ProjectLanguage::Cpp,
                compilation_database: None,
            },
        )
        .unwrap();
    let main = named_symbol(&mut workflow, &path, "main");
    let origin = workflow.add_symbol(main, Point::default()).unwrap();
    let later = workflow
        .expand_definition(&origin, position(&path, "options() +"))
        .unwrap()
        .remove(0);
    let earlier = workflow
        .expand_definition(&origin, position(&path, "first();"))
        .unwrap()
        .remove(0);
    let cards = &workflow.session().cards;
    let earlier = cards.iter().find(|card| card.id == earlier).unwrap();
    let later_card = cards.iter().find(|card| card.id == later).unwrap();
    assert_eq!(earlier.position.x, later_card.position.x);
    assert!(earlier.position.y + earlier.display_height() < later_card.position.y);
    assert_eq!(earlier.source.context.len(), 3);
    assert_eq!(later_card.source.context.len(), 3);
    let target = workflow
        .expand_definition(&later, position(&path, "Project {"))
        .unwrap()
        .remove(0);
    assert_eq!(
        workflow
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
    workflow.expand_context(&later, 2).unwrap();
    let card = workflow
        .session()
        .cards
        .iter()
        .find(|card| card.id == later)
        .unwrap();
    assert!(card.source.export_context()[2].code.contains("int first()"));
    assert!(
        card.source
            .contains_display_position(position(&path, "first() const"))
    );
    assert_eq!(
        card.source.display_row(position(&path, "options() const")),
        Some(6)
    );
    let expanded = card.source.clone();
    workflow.collapse_context(&later, 2).unwrap();
    let folded = &workflow
        .session()
        .cards
        .iter()
        .find(|card| card.id == later)
        .unwrap()
        .source;
    assert_eq!(folded.context[2].code, "        class Project {");
    assert!(!folded.contains_display_position(position(&path, "first() const")));
    assert!(folded.export_expanded().is_empty());
    workflow.expand_context(&later, 2).unwrap();
    assert_eq!(
        workflow
            .session()
            .cards
            .iter()
            .find(|card| card.id == later)
            .unwrap()
            .source,
        expanded
    );
    workflow.session().validate().unwrap();
}
