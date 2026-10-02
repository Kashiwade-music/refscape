use crate::{
    document::atomic_write,
    session::{self, JsonSessionRepository, v1},
    theme::{load_theme, save_theme},
};
use refscape_application::{
    ApplicationSnapshot as Session, ImportedSession, PersistableSession, SessionRepository,
};
use refscape_model::{
    CodeCard, Connection, ConnectionKind, Point, Position, ProjectLanguage, ProjectOpenOptions,
    Region, SourceDocument, SourceRange, Symbol, Theme, Viewport,
};
use std::{
    env, fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct TestDirectory(PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let id = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path =
            env::temp_dir().join(format!("refscape-storage-test-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn save(path: &std::path::Path, session: &Session) -> Result<(), refscape_model::RefscapeError> {
    JsonSessionRepository.save(
        path,
        &PersistableSession {
            snapshot: Arc::new(session.clone()),
            epoch: 1,
            revision: 1,
        },
    )
}
fn load(path: &std::path::Path) -> Result<Session, refscape_model::RefscapeError> {
    JsonSessionRepository
        .load(path)
        .map(|loaded| loaded.snapshot)
}
fn document(session: &Session) -> serde_json::Value {
    serde_json::to_value(session::streaming::SessionView(session)).unwrap()
}
fn edit_source(card: &mut CodeCard, edit: impl FnOnce(&mut SourceDocument)) {
    let mut source = card.source.to_document();
    edit(&mut source);
    card.source = source.try_into().unwrap();
}
#[test]
fn python_session_preserves_language_without_a_version_change() {
    let directory = TestDirectory::new();
    let path = directory.path("python-session.json");
    let mut session = Session::new(directory.0.clone());
    session.project_options.language = ProjectLanguage::Python;
    save(&path, &session).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(document["version"], 1);
    assert_eq!(document["project_options"]["language"], "python");
    assert_eq!(load(&path).unwrap(), session);
}
#[test]
fn theme_roundtrip_preserves_custom_colors() {
    let directory = TestDirectory::new();
    let path = directory.path("theme.json");
    let mut theme = Theme::dark();
    theme.name = "Custom theme".into();
    theme.palette.accent = "#bb55ff".into();
    save_theme(&path, &theme).unwrap();
    assert_eq!(load_theme(&path).unwrap(), theme);
}
#[test]
fn legacy_sessions_default_to_automatic_language_detection() {
    let directory = TestDirectory::new();
    let path = directory.path("session.json");
    let session = Session::new(directory.0.clone());
    let mut legacy = document(&session);
    legacy.as_object_mut().unwrap().remove("project_options");
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let restored = load(&path).unwrap();
    assert_eq!(restored, session);
    assert_eq!(restored.project_options, ProjectOpenOptions::default());
}
#[test]
fn obsolete_layout_settings_are_ignored_and_removed_when_resaved() {
    let directory = TestDirectory::new();
    let path = directory.path("session.json");
    let session = Session::new(directory.0.clone());
    for obsolete in [
        serde_json::json!({"auto_compact_enabled":true,"auto_compact_min_reduction_percent":30}),
        serde_json::json!({"auto_compact_enabled":false,"auto_compact_min_reduction_percent":0}),
        serde_json::json!({"auto_compact_enabled":"unknown","auto_compact_min_reduction_percent":999}),
        serde_json::json!(null),
    ] {
        let mut legacy = document(&session);
        legacy["layout_settings"] = obsolete;
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let restored = load(&path).unwrap();
        assert_eq!(restored, session);
        save(&path, &restored).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(saved.get("layout_settings").is_none());
        assert_eq!(saved["version"], 1);
    }
}
#[test]
fn partially_specified_project_options_use_field_defaults() {
    for (name, expected) in [
        ("python", ProjectLanguage::Python),
        ("typescript", ProjectLanguage::TypeScript),
        ("cpp", ProjectLanguage::Cpp),
    ] {
        let wire: v1::ProjectOptions =
            serde_json::from_value(serde_json::json!({"language":name})).unwrap();
        let options: ProjectOpenOptions = wire.into();
        assert_eq!(options.language, expected);
        assert_eq!(options.compilation_database, None);
        assert_eq!(
            serde_json::to_value(v1::ProjectOptions::from(&options)).unwrap()["language"],
            name
        );
    }
    let options: ProjectOpenOptions = serde_json::from_str::<v1::ProjectOptions>("{}")
        .unwrap()
        .into();
    assert_eq!(options, ProjectOpenOptions::default());
}
fn fixture(directory: &TestDirectory) -> Session {
    let mut session = Session::new(directory.0.clone());
    session.project_options = ProjectOpenOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(directory.path("out/debug/compile_commands.json")),
    };
    let range = SourceRange {
        start: Position::new(0, 0),
        end: Position::new(1, 0),
    };
    for (id, x) in [("main", -30.5), ("run", 700.25)] {
        Arc::make_mut(&mut session.cards).push(CodeCard {
            id: id.into(),
            source: SourceDocument {
                expanded: vec![],
                folded: vec![],
                context: vec![],
                code_start: None,
                symbol: Symbol::file(directory.path(&format!("{id}.rs")), range),
                code: format!("fn {id}() {{}}\n"),
                tokens: vec![],
            }
            .try_into()
            .unwrap(),
            position: Point::new(x, 125.).try_into().unwrap(),
            width: 600.,
            height: 320.,
        });
    }
    edit_source(&mut Arc::make_mut(&mut session.cards)[1], |source| {
        source.symbol.range = SourceRange {
            start: Position::new(5, 4),
            end: Position::new(6, 0),
        };
        source.symbol.selection_range = source.symbol.range;
        source.code_start = Some(Position::new(5, 0));
        source.code = "    fn run() {}".into();
        source.context.push(refscape_model::SourceContext {
            start_line: 1,
            code: "impl Sample {".into(),
        });
        source.folded.push(refscape_model::SourceContext {
            start_line: 2,
            code: "    fn first() {}\n\n\n".into(),
        });
    });
    for (id, kind) in [
        ("main-to-run", ConnectionKind::Definition),
        ("main-to-type", ConnectionKind::TypeDefinition),
    ] {
        Arc::make_mut(&mut session.connections).push(Connection {
            id: id.into(),
            from: "main".into(),
            to: "run".into(),
            kind,
            source: Position::new(0, 3),
        });
    }
    Arc::make_mut(&mut session.regions).push(Region {
        kind: refscape_model::RegionKind::Crate,
        id: "crate".into(),
        label: "Example crate".into(),
        path: directory.0.clone(),
        card_ids: vec!["main".into(), "run".into()],
    });
    session.viewport = Viewport {
        offset: Point::new(-220., 100.).try_into().unwrap(),
        zoom: 0.75,
    };
    session.theme = Theme::light();
    session
}
#[test]
fn session_roundtrip_preserves_canvas_code_connections_and_theme() {
    let directory = TestDirectory::new();
    let path = directory.path("nested/exploration.json");
    let mut session = fixture(&directory);
    let mut legacy_source = serde_json::to_value(v1::SourceDocument::from(
        &session.cards[0].source.to_document(),
    ))
    .unwrap();
    for name in ["context", "code_start", "folded", "expanded"] {
        legacy_source.as_object_mut().unwrap().remove(name);
    }
    let old: SourceDocument = serde_json::from_value::<v1::SourceDocument>(legacy_source)
        .unwrap()
        .into();
    assert!(old.context.is_empty());
    assert!(old.code_start.is_none());
    assert!(old.folded.is_empty());
    assert!(old.expanded.is_empty());
    let mut legacy = document(&session);
    legacy["layout_settings"] =
        serde_json::json!({"auto_compact_enabled":true,"auto_compact_min_reduction_percent":30});
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(load(&path).unwrap(), session);
    save(&path, &session).unwrap();
    assert_eq!(load(&path).unwrap(), session);
    Arc::make_mut(&mut session.cards)[1]
        .source
        .toggle_fold(0)
        .unwrap();
    save(&path, &session).unwrap();
    let restored = load(&path).unwrap();
    assert_eq!(restored, session);
    assert_eq!(document(&restored), document(&session));
    Arc::make_mut(&mut session.cards)[0].position = Point::new(999., -222.).try_into().unwrap();
    save(&path, &session).unwrap();
    assert_eq!(load(&path).unwrap(), session);
}
#[test]
fn invalid_session_does_not_overwrite_last_valid_session() {
    let directory = TestDirectory::new();
    let path = directory.path("session.json");
    let session = Session::new(directory.0.clone());
    save(&path, &session).unwrap();
    let mut invalid = session.clone();
    invalid.viewport.zoom = f32::NAN;
    assert!(save(&path, &invalid).is_err());
    assert_eq!(load(&path).unwrap(), session);
}
#[test]
fn rejects_malformed_and_future_formats() {
    let directory = TestDirectory::new();
    let path = directory.path("session.json");
    fs::write(&path, "{not JSON").unwrap();
    assert_eq!(
        load(&path).unwrap_err().kind,
        refscape_model::ErrorKind::InvalidData
    );
    fs::write(&path, r#"{"version":999,"new_schema":true}"#).unwrap();
    assert_eq!(
        load(&path).unwrap_err().kind,
        refscape_model::ErrorKind::Unsupported
    );
    assert!(
        load(&path)
            .unwrap_err()
            .to_string()
            .contains("unsupported session version 999")
    );
    fs::write(&path, r#"{"version":"1"}"#).unwrap();
    assert!(load(&path).is_err());
    fs::write(&path, r#"{"version":999}"#).unwrap();
    assert!(load_theme(&path).is_err());
}

#[test]
fn repository_io_failures_keep_typed_kind_path_and_save_phase() {
    let directory = TestDirectory::new();
    let missing = directory.path("missing.json");
    let error = load(&missing).unwrap_err();
    assert_eq!(error.kind, refscape_model::ErrorKind::Io);
    assert_eq!(error.path.as_deref(), Some(missing.as_path()));
    let destination = directory.path("directory-instead-of-file");
    fs::create_dir(&destination).unwrap();
    let error = save(&destination, &fixture(&directory)).unwrap_err();
    assert_eq!(error.kind, refscape_model::ErrorKind::Io);
    assert_eq!(error.path.as_deref(), Some(destination.as_path()));
    assert_eq!(error.operation.as_deref(), Some("write-before-replace"));
    assert!(destination.is_dir());
}
#[test]
fn invalid_theme_cannot_replace_a_previous_theme() {
    let directory = TestDirectory::new();
    let path = directory.path("theme.json");
    let theme = Theme::light();
    save_theme(&path, &theme).unwrap();
    let mut invalid = theme.clone();
    invalid.palette.accent = "not a color".into();
    assert!(save_theme(&path, &invalid).is_err());
    assert_eq!(load_theme(&path).unwrap(), theme);
}
#[test]
fn interrupted_staging_file_does_not_replace_committed_document() {
    let directory = TestDirectory::new();
    let path = directory.path("session.json");
    let session = fixture(&directory);
    save(&path, &session).unwrap();
    fs::write(directory.path("session.json.123.456.tmp"), "{partial").unwrap();
    assert_eq!(load(&path).unwrap(), session);
    save(&path, &session).unwrap();
    assert_eq!(load(&path).unwrap(), session);
}
#[test]
fn failed_commit_cleans_up_staging_file() {
    let directory = TestDirectory::new();
    let path = directory.path("existing-directory");
    fs::create_dir(&path).unwrap();
    assert!(atomic_write(&path, b"payload").is_err());
    assert!(path.is_dir());
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
}
#[test]
fn loaded_bytes_are_fixed_and_do_not_follow_file_changes() {
    let directory = TestDirectory::new();
    let mut first = fixture(&directory);
    let bytes = serde_json::to_vec(&document(&first)).unwrap();
    let loaded: ImportedSession = session::decode_v1(&bytes).unwrap();
    first.theme = Theme::dark();
    assert_ne!(loaded.snapshot, first);
    assert_eq!(loaded.snapshot.theme, Theme::light());
}

const FULL_V1_GOLDEN: &[u8] = include_bytes!("../tests/fixtures/session-v1-full.json");
const LEGACY_V1_GOLDEN: &[u8] = include_bytes!("../tests/fixtures/session-v1-legacy.json");

#[test]
fn fixed_v1_golden_preserves_every_json_value_and_saved_order() {
    let original: serde_json::Value = serde_json::from_slice(FULL_V1_GOLDEN).unwrap();
    let loaded = session::decode_v1(FULL_V1_GOLDEN).unwrap().snapshot;
    assert_eq!(document(&loaded), original);
    assert_eq!(
        loaded.cards[0].id.as_str(),
        "saved:日本語:second-before-first"
    );
    assert_eq!(loaded.cards[1].id.as_str(), "saved:alpha");
    assert_eq!(loaded.connections[0].id.as_str(), "edge:reference-first");
    assert_eq!(
        loaded.cards[0].source.code.as_ref().as_bytes(),
        "\tfn 日本語() {😀}\r\n".as_bytes()
    );
    assert_eq!(
        loaded.cards[1].source.display_lines()[1].text,
        "    fn old() {}"
    );
    assert_eq!(loaded.theme.name, "Golden custom");
    let directory = TestDirectory::new();
    let path = directory.path("fixed.json");
    save(&path, &loaded).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        original
    );
    assert_eq!(load(&path).unwrap(), loaded);
}

#[test]
fn fixed_legacy_golden_defaults_only_missing_fields_and_keeps_opaque_context() {
    let mut expected: serde_json::Value = serde_json::from_slice(LEGACY_V1_GOLDEN).unwrap();
    expected.as_object_mut().unwrap().remove("layout_settings");
    expected["project_options"] =
        serde_json::json!({"language":"auto","compilation_database":null});
    expected["regions"] = serde_json::json!([]);
    for card in expected["cards"].as_array_mut().unwrap() {
        let source = card["source"].as_object_mut().unwrap();
        source.insert("tokens".into(), serde_json::json!([]));
        source.entry("context").or_insert(serde_json::json!([]));
        source.insert("code_start".into(), serde_json::Value::Null);
        source.insert("folded".into(), serde_json::json!([]));
        source.insert("expanded".into(), serde_json::json!([]));
        source["symbol"]
            .as_object_mut()
            .unwrap()
            .insert("children".into(), serde_json::json!([]));
    }
    let loaded = session::decode_v1(LEGACY_V1_GOLDEN).unwrap().snapshot;
    assert_eq!(document(&loaded), expected);
    let source = &loaded.cards[0].source;
    let rows = source.display_lines();
    assert_eq!(
        rows.iter().map(|row| row.text.as_ref()).collect::<Vec<_>>(),
        ["mod legacy {", "    // already visible", "fn old() {}"]
    );
    assert!(source.folded_range(0).is_none());
    assert_eq!(loaded.project_options, ProjectOpenOptions::default());
    assert_eq!(
        session::decode_v1(&serde_json::to_vec(&expected).unwrap())
            .unwrap()
            .snapshot,
        loaded
    );
}

#[test]
fn stored_card_bytes_do_not_follow_changed_or_deleted_source_files() {
    let directory = TestDirectory::new();
    let path = directory.path("snapshot.json");
    let source_path = directory.path("live.cpp");
    let mut expected: serde_json::Value = serde_json::from_slice(FULL_V1_GOLDEN).unwrap();
    expected["project_root"] = serde_json::json!(directory.0);
    expected["cards"][0]["source"]["symbol"]["path"] = serde_json::json!(source_path);
    fs::write(&path, serde_json::to_vec(&expected).unwrap()).unwrap();
    fs::write(&source_path, "completely different live source").unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(document(&loaded), expected);
    fs::write(&source_path, "another external edit").unwrap();
    save(&path, &loaded).unwrap();
    assert_eq!(document(&load(&path).unwrap()), expected);
    fs::remove_file(&source_path).unwrap();
    assert_eq!(document(&load(&path).unwrap()), expected);
}
#[test]
fn overflow_source_is_rejected_without_display_panic() {
    let directory = TestDirectory::new();
    let mut wire = document(&fixture(&directory));
    wire["cards"][0]["source"]["symbol"]["range"] = serde_json::json!({"start":{"line":u32::MAX,"character":0},"end":{"line":u32::MAX,"character":0}});
    wire["cards"][0]["source"]["symbol"]["selection_range"] =
        wire["cards"][0]["source"]["symbol"]["range"].clone();
    wire["cards"][0]["source"]["code"] = serde_json::json!("a\nb");
    assert!(session::decode_v1(&serde_json::to_vec(&wire).unwrap()).is_err());
}
