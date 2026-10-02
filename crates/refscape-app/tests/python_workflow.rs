//! Python navigation and canvas persistence through the composition root.
#[path = "common/workflow.rs"]
mod common;
use common::Workflow;
use refscape_language::LanguageBackend;
use refscape_model::{ConnectionKind, Point, Position, ProjectLanguage, ProjectOpenOptions};
use refscape_storage::session::JsonSessionRepository;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn position(path: &Path, line_text: &str, token: &str) -> Position {
    let text = fs::read_to_string(path).unwrap();
    let (line, source) = text
        .lines()
        .enumerate()
        .find(|(_, source)| source.contains(line_text))
        .unwrap();
    Position::new(
        line as u32,
        source[..source.find(token).unwrap()].encode_utf16().count() as u32,
    )
}

#[test]
#[ignore = "requires pip install basedpyright or npm install -g basedpyright"]
fn python_canvas_navigation_variable_types_context_and_session_restore() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/python-demo")
        .canonicalize()
        .unwrap();
    let backend = || LanguageBackend::default();
    let mut explorer = Workflow::new(backend(), JsonSessionRepository);
    explorer
        .open_project(&root, &ProjectOpenOptions::default())
        .unwrap();
    assert_eq!(
        explorer.session().project_options.language,
        ProjectLanguage::Python
    );
    assert_eq!(explorer.files().unwrap().len(), 3);

    let main_path = root.join("main.py");
    let main = explorer
        .symbols(&main_path)
        .unwrap()
        .into_iter()
        .find(|symbol| symbol.name == "main")
        .unwrap();
    let main = explorer.add_symbol(main, Point::new(20.0, 20.0)).unwrap();
    for (line, token, target_file, target_name) in [
        (
            "config: Config = load_config()",
            "load_config",
            "model.py",
            "load_config",
        ),
        ("print(run(config))", "run", "pipeline.py", "run"),
    ] {
        let at = position(&main_path, line, token);
        let targets = explorer.expand_definition(&main, at).unwrap();
        assert!(
            targets
                .iter()
                .any(|id| explorer.session().cards.iter().any(|card| {
                    &card.id == id
                        && card.source.symbol.path.ends_with(target_file)
                        && card.source.symbol.name == target_name
                }))
        );
        assert!(explorer.hover(&main, at).unwrap().is_some());
    }

    let run_symbol = explorer
        .search("run")
        .unwrap()
        .into_iter()
        .find(|symbol| symbol.name == "run" && symbol.path.ends_with("pipeline.py"))
        .unwrap();
    let run = explorer.add_symbol(run_symbol, Point::default()).unwrap();
    let variable = position(
        &root.join("pipeline.py"),
        "counter: Counter = build_counter",
        "counter",
    );
    let inspection = explorer
        .inspect_variable(&run, variable)
        .unwrap()
        .expect("Python variables have semantic tokens");
    assert!(inspection.highlights.len() >= 2);
    assert!(inspection.description.is_some());
    let types = explorer
        .toggle_type_definition(&run, variable)
        .unwrap()
        .unwrap();
    assert!(
        types
            .iter()
            .any(|id| explorer.session().cards.iter().any(|card| {
                &card.id == id
                    && card.source.symbol.path.ends_with("model.py")
                    && card.source.symbol.name == "Counter"
            }))
    );

    let load = explorer
        .search("load_config")
        .unwrap()
        .into_iter()
        .find(|symbol| symbol.name == "load_config")
        .unwrap();
    let declaration = load.selection_range.start;
    let load = explorer.add_symbol(load, Point::default()).unwrap();
    let callers = explorer.expand_references(&load, declaration).unwrap();
    for file in ["main.py", "pipeline.py"] {
        assert!(callers.iter().any(|id| {
            explorer
                .session()
                .cards
                .iter()
                .any(|card| &card.id == id && card.source.symbol.path.ends_with(file))
        }));
    }

    for (name, container) in [("increment", "Counter"), ("annotate", "summarize")] {
        let symbol = explorer
            .search(name)
            .unwrap()
            .into_iter()
            .find(|symbol| symbol.name == name)
            .unwrap();
        let id = explorer.add_symbol(symbol, Point::default()).unwrap();
        let card = explorer
            .session()
            .cards
            .iter()
            .find(|card| card.id == id)
            .unwrap();
        assert!(
            card.source
                .context
                .iter()
                .any(|context| context.code.contains(container))
        );
        assert!(!card.source.tokens.is_empty());
        assert!(!card.source.export_folded().is_empty());
        explorer.expand_context(&id, 0).unwrap();
        assert!(
            !explorer
                .session()
                .cards
                .iter()
                .find(|card| card.id == id)
                .unwrap()
                .source
                .export_expanded()
                .is_empty()
        );
    }
    for kind in [
        ConnectionKind::Definition,
        ConnectionKind::Reference,
        ConnectionKind::TypeDefinition,
    ] {
        assert!(
            explorer
                .session()
                .connections
                .iter()
                .any(|edge| edge.kind == kind)
        );
    }
    explorer.session().validate().unwrap();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let session_path = std::env::temp_dir().join(format!(
        "refscape-python-session-{}-{unique}.json",
        std::process::id()
    ));
    explorer.save_session(&session_path).unwrap();
    let saved = explorer.session().clone();
    drop(explorer);
    let mut restored = Workflow::new(backend(), JsonSessionRepository);
    restored
        .load_project_session(&session_path, &root, &ProjectOpenOptions::default())
        .unwrap();
    assert_eq!(restored.session(), &saved);
    restored.session().validate().unwrap();
    fs::remove_file(session_path).unwrap();
}
