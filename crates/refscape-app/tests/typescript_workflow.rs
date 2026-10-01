//! Real TypeScript/React/React Native navigation through the composition root.
use refscape_application::explorer::Explorer;
use refscape_language::LanguageBackend;
use refscape_model::{ConnectionKind, Point, Position, ProjectLanguage, ProjectOptions};
use refscape_storage::session::JsonSessionRepository;
use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[test]
#[ignore = "requires Node.js and npm install in examples/typescript-demo"]
fn react_native_canvas_navigation_context_and_session_restore() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/typescript-demo")
        .canonicalize()
        .unwrap();
    let mut explorer = Explorer::new(
        LanguageBackend::default().with_timeout(Duration::from_secs(45)),
        JsonSessionRepository,
    );
    explorer
        .open_project(&root, &ProjectOptions::default())
        .unwrap();
    assert_eq!(
        explorer.session().project_options.language,
        ProjectLanguage::TypeScript
    );
    for (file, name, target) in [
        ("src/App.tsx", "App", "src/CounterView.tsx"),
        ("src/Legacy.jsx", "Legacy", "src/CounterView.tsx"),
        (
            "mobile/src/App.tsx",
            "NativeApp",
            "mobile/src/CounterView.native.tsx",
        ),
        (
            "mobile/src/Legacy.jsx",
            "NativeLegacy",
            "mobile/src/CounterView.native.tsx",
        ),
    ] {
        let path = root.join(file);
        let symbol = explorer
            .symbols(&path)
            .unwrap()
            .into_iter()
            .find(|symbol| symbol.name == name)
            .unwrap();
        let id = explorer.add_symbol(symbol, Point::new(20.0, 20.0)).unwrap();
        let code = fs::read_to_string(&path).unwrap();
        let (line, text) = code
            .lines()
            .enumerate()
            .find(|(_, text)| text.contains("return <CounterView"))
            .unwrap();
        let position = Position::new(
            line as u32,
            text[..text.find("CounterView").unwrap()]
                .encode_utf16()
                .count() as u32,
        );
        let targets = explorer.expand_definition(&id, position).unwrap();
        let view =
            targets
                .iter()
                .find(|target_id| {
                    explorer.session().cards.iter().any(|card| {
                        &card.id == *target_id && card.source.symbol.path.ends_with(target)
                    })
                })
                .unwrap()
                .clone();
        let card = explorer
            .session()
            .cards
            .iter()
            .find(|card| card.id == view)
            .unwrap();
        let token = card
            .source
            .tokens
            .iter()
            .find(|token| token.kind == "parameter")
            .unwrap();
        let variable = Position::new(token.line, token.start);
        let inspection = explorer.inspect_variable(&view, variable).unwrap().unwrap();
        assert!(inspection.highlights.len() >= 2);
        assert!(inspection.description.is_some());
        if !explorer
            .session()
            .connections
            .iter()
            .any(|edge| edge.from == view && edge.kind == ConnectionKind::TypeDefinition)
        {
            assert!(
                explorer
                    .toggle_type_definition(&view, variable)
                    .unwrap()
                    .is_some()
            );
        }
        assert!(explorer.hover(&id, position).unwrap().is_some());
    }
    let method = explorer
        .search("increment")
        .unwrap()
        .into_iter()
        .find(|symbol| symbol.name == "increment")
        .unwrap();
    let method = explorer.add_symbol(method, Point::default()).unwrap();
    let source = &explorer
        .session()
        .cards
        .iter()
        .find(|card| card.id == method)
        .unwrap()
        .source;
    assert_eq!(source.context.len(), 2);
    assert!(!source.folded.is_empty());
    explorer.expand_context(&method, 1).unwrap();
    assert!(
        !explorer
            .session()
            .cards
            .iter()
            .find(|card| card.id == method)
            .unwrap()
            .source
            .expanded
            .is_empty()
    );
    explorer.session().validate().unwrap();
    assert!(
        explorer
            .session()
            .connections
            .iter()
            .any(|edge| edge.kind == ConnectionKind::TypeDefinition)
    );
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let session_path = std::env::temp_dir().join(format!(
        "refscape-ts-session-{}-{unique}.json",
        std::process::id()
    ));
    explorer.save_session(&session_path).unwrap();
    let saved = explorer.session().clone();
    drop(explorer);
    let mut restored = Explorer::new(
        LanguageBackend::default().with_timeout(Duration::from_secs(45)),
        JsonSessionRepository,
    );
    restored
        .load_project_session(&session_path, &root, &ProjectOptions::default())
        .unwrap();
    assert_eq!(restored.session(), &saved);
    restored.session().validate().unwrap();
    fs::remove_file(session_path).unwrap();
}
