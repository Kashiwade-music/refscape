//! Complete adapter composition; opt-in because it starts a real rust-analyzer.

use std::{
    env, fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use refscape_application::Explorer;
use refscape_language::RustAnalyzer;
use refscape_model::{ConnectionKind, Point, Position, Theme};
use refscape_storage::JsonSessionRepository;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            env::temp_dir().join(format!("refscape-workflow-{}-{unique}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"refscape_workflow_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n",
        )
        .unwrap();
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
        Self {
            root: root.canonicalize().unwrap(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Restrict recursive cleanup to this test's uniquely named temp fixture.
        let temp = env::temp_dir().canonicalize().unwrap();
        if self.root.parent() == Some(temp.as_path())
            && self
                .root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .starts_with("refscape-workflow-")
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

#[test]
#[ignore = "requires rust-analyzer; run cargo test -p refscape-app --test workflow -- --ignored"]
fn real_project_navigation_and_named_session_restore() {
    let fixture = Fixture::new();
    let language = RustAnalyzer::default().with_timeout(Duration::from_secs(60));
    let mut explorer = Explorer::new(language, JsonSessionRepository);
    explorer.open_project(&fixture.root).unwrap();
    assert_eq!(explorer.files().unwrap().len(), 2);

    let lib = fixture.root.join("src/lib.rs");
    let helper = fixture.root.join("src/helper.rs");
    let entry = explorer
        .symbols(&lib)
        .unwrap()
        .into_iter()
        .find(|symbol| symbol.name == "entry")
        .expect("entry returned by rust-analyzer");
    let origin = explorer.add_symbol(entry, Point::new(40.0, 50.0)).unwrap();
    let call = Position::new(2, 13);
    let targets = explorer.expand_definition(&origin, call).unwrap();
    assert_eq!(targets.len(), 1);
    let answer = targets[0].clone();
    let answer_card = explorer
        .session()
        .cards
        .iter()
        .find(|card| card.id == answer)
        .unwrap();
    assert_eq!(answer_card.source.symbol.path, helper);
    assert_eq!(answer_card.source.symbol.name, "answer");
    assert!(answer_card.source.code.contains("42"));
    assert!(
        answer_card
            .source
            .tokens
            .iter()
            .any(|token| token.kind == "function")
    );
    let answer_name = answer_card.source.symbol.selection_range.start;
    let references = explorer.expand_references(&answer, answer_name).unwrap();
    assert!(references.contains(&origin));
    assert_eq!(explorer.session().cards.len(), 2);
    assert_eq!(explorer.session().connections.len(), 2);
    assert!(
        explorer
            .session()
            .connections
            .iter()
            .any(|edge| edge.kind == ConnectionKind::Reference
                && edge.from == answer
                && edge.to == origin)
    );

    // Repeating the same navigation reuses cards and their existing edges.
    assert_eq!(explorer.expand_definition(&origin, call).unwrap(), targets);
    explorer.expand_references(&answer, answer_name).unwrap();
    assert_eq!(explorer.session().cards.len(), 2);
    assert_eq!(explorer.session().connections.len(), 2);

    let whole_file = explorer.add_file(&lib, Point::new(80.0, 600.0)).unwrap();
    let file_card = explorer
        .session()
        .cards
        .iter()
        .find(|card| card.id == whole_file)
        .unwrap();
    assert_eq!(file_card.source.code, fs::read_to_string(&lib).unwrap());
    assert_eq!(file_card.source.symbol.range.start, Position::new(0, 0));
    assert_eq!(file_card.source.symbol.range.end, Position::new(4, 0));
    assert_eq!(
        explorer.add_file(&lib, Point::default()).unwrap(),
        whole_file
    );
    assert_eq!(explorer.session().cards.len(), 3);

    explorer
        .move_card(&origin, Point::new(-125.0, 315.0))
        .unwrap();
    explorer.pan(Point::new(170.0, -45.0)).unwrap();
    let cursor = Point::new(450.0, 250.0);
    let world = explorer.session().viewport.screen_to_world(cursor);
    explorer.zoom(1.5, cursor).unwrap();
    assert_eq!(explorer.session().viewport.world_to_screen(world), cursor);
    explorer.set_theme(Theme::light()).unwrap();

    let named_session = fixture.root.join("sessions/review.json");
    let saved = explorer.session().clone();
    explorer.save_session(&named_session).unwrap();
    explorer.remove_card(&answer).unwrap();
    assert!(explorer.session().connections.is_empty());
    assert!(
        explorer
            .session()
            .regions
            .iter()
            .all(|region| !region.card_ids.contains(&answer))
    );
    explorer.pan(Point::new(200.0, 200.0)).unwrap();
    explorer.set_theme(Theme::dark()).unwrap();
    explorer.load_session(&named_session).unwrap();
    assert_eq!(explorer.session(), &saved);
    explorer.session().validate().unwrap();
    assert!(
        explorer
            .search("answer")
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "answer")
    );
    // Stop the server before Fixture removes its project and target directory.
    drop(explorer);
}
