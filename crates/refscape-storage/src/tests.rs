use std::{
    env, fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use refscape_application::ports::SessionRepository;
use refscape_model::{
    CodeCard, Connection, ConnectionKind, Point, Position, ProjectLanguage, ProjectOptions, Region,
    Session, SourceDocument, SourceRange, Symbol, Theme, Viewport,
};

use crate::{
    document::atomic_write,
    session::JsonSessionRepository,
    settings::{Settings, load_settings, save_settings},
    theme::{load_theme, save_theme},
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let id = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path =
            env::temp_dir().join(format!("refscape-storage-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
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

#[test]
fn settings_roundtrip_and_first_launch_defaults() {
    let directory = TestDirectory::new();
    let path = directory.path("config/settings.json");
    assert_eq!(load_settings(&path).unwrap(), Settings::default());
    let settings = Settings {
        last_project: Some(PathBuf::from("workspace/日本語")),
        theme_file: Some(PathBuf::from("themes/custom.json")),
        rust_analyzer_path: Some(PathBuf::from("tools/rust-analyzer")),
        clangd_path: Some(PathBuf::from("tools/clangd")),
        typescript_language_server_path: Some(PathBuf::from(
            "tools/typescript-language-server/lib/cli.mjs",
        )),
        pyright_path: Some(PathBuf::from("tools/pyright/dist/langserver.index.js")),
        ..Settings::default()
    };
    save_settings(&path, &settings).unwrap();
    assert_eq!(load_settings(&path).unwrap(), settings);
    save_settings(&path, &Settings::default()).unwrap();
    assert_eq!(load_settings(&path).unwrap(), Settings::default());
}

#[test]
fn pre_python_settings_default_server_path_and_preserve_existing_overrides() {
    let directory = TestDirectory::new();
    let path = directory.path("settings.json");
    let mut previous = serde_json::to_value(Settings {
        clangd_path: Some("tools/clangd".into()),
        typescript_language_server_path: Some("tools/typescript-language-server".into()),
        ..Settings::default()
    })
    .unwrap();
    previous.as_object_mut().unwrap().remove("pyright_path");
    fs::write(&path, serde_json::to_vec(&previous).unwrap()).unwrap();
    let settings = load_settings(&path).unwrap();
    assert_eq!(settings.pyright_path, None);
    assert_eq!(settings.clangd_path, Some("tools/clangd".into()));
    assert_eq!(
        settings.typescript_language_server_path,
        Some("tools/typescript-language-server".into())
    );
}

#[test]
fn python_session_preserves_language_without_a_version_change() {
    let directory = TestDirectory::new();
    let path = directory.path("python-session.json");
    let mut session = Session::new(directory.0.clone());
    session.project_options.language = ProjectLanguage::Python;
    JsonSessionRepository.save(&path, &session).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(document["version"], 1);
    assert_eq!(document["project_options"]["language"], "python");
    assert_eq!(JsonSessionRepository.load(&path).unwrap(), session);
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
fn legacy_settings_and_sessions_default_to_automatic_language_detection() {
    let directory = TestDirectory::new();
    let settings_path = directory.path("settings.json");
    fs::write(
        &settings_path,
        r#"{"version":1,"last_project":null,"theme_file":null,"rust_analyzer_path":null}"#,
    )
    .unwrap();
    assert_eq!(load_settings(&settings_path).unwrap(), Settings::default());

    let session_path = directory.path("session.json");
    let session = Session::new(directory.0.clone());
    let mut legacy = serde_json::to_value(&session).unwrap();
    legacy.as_object_mut().unwrap().remove("project_options");
    fs::write(&session_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let restored = JsonSessionRepository.load(&session_path).unwrap();
    assert_eq!(restored, session);
    assert_eq!(restored.version, 1);
    assert_eq!(restored.project_options, ProjectOptions::default());
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
        let mut legacy = serde_json::to_value(&session).unwrap();
        legacy["layout_settings"] = obsolete;
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let restored = JsonSessionRepository.load(&path).unwrap();
        assert_eq!(restored, session);
        assert_eq!(restored.version, 1);
        JsonSessionRepository.save(&path, &restored).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(saved.get("layout_settings").is_none());
    }
}

#[test]
fn partially_specified_project_options_use_field_defaults() {
    let options: ProjectOptions = serde_json::from_str(r#"{"language":"python"}"#).unwrap();
    assert_eq!(options.language, ProjectLanguage::Python);
    assert_eq!(options.compilation_database, None);
    assert_eq!(
        serde_json::to_value(&options).unwrap()["language"],
        "python"
    );
    let options: ProjectOptions = serde_json::from_str(r#"{"language":"typescript"}"#).unwrap();
    assert_eq!(options.language, ProjectLanguage::TypeScript);
    assert_eq!(
        serde_json::to_value(&options).unwrap()["language"],
        "typescript"
    );
    let options: ProjectOptions = serde_json::from_str(r#"{"language":"cpp"}"#).unwrap();
    assert_eq!(options.language, ProjectLanguage::Cpp);
    assert_eq!(options.compilation_database, None);
    let options: ProjectOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(options, ProjectOptions::default());
}

#[test]
fn session_roundtrip_preserves_canvas_code_connections_and_theme() {
    let directory = TestDirectory::new();
    let path = directory.path("nested/exploration.json");
    let mut session = Session::new(directory.0.clone());
    session.project_options = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(directory.path("out/debug/compile_commands.json")),
    };
    let range = SourceRange {
        start: Position::new(0, 0),
        end: Position::new(1, 0),
    };
    for (id, x) in [("main", -30.5), ("run", 700.25)] {
        session.cards.push(CodeCard {
            id: id.into(),
            source: SourceDocument {
                expanded: Vec::new(),
                folded: Vec::new(),
                context: Vec::new(),
                code_start: None,
                symbol: Symbol::file(directory.path(&format!("{id}.rs")), range),
                code: format!("fn {id}() {{}}\n"),
                tokens: vec![],
            },
            position: Point::new(x, 125.0),
            width: 600.0,
            height: 320.0,
        });
    }
    // Declaration context and indentation survive saving; older snapshots default to no context.
    let legacy = serde_json::to_value(&session.cards[0].source).unwrap();
    let mut legacy = legacy.as_object().unwrap().clone();
    legacy.remove("context");
    legacy.remove("code_start");
    legacy.remove("folded");
    legacy.remove("expanded");
    let old_source: SourceDocument =
        serde_json::from_value(serde_json::Value::Object(legacy)).unwrap();
    assert!(old_source.context.is_empty());
    assert_eq!(old_source.code_start, None);
    assert!(old_source.folded.is_empty());
    assert!(old_source.expanded.is_empty());
    session.cards[1].source.symbol.range.start = Position::new(5, 4);
    session.cards[1].source.symbol.range.end = Position::new(6, 0);
    session.cards[1].source.symbol.selection_range = session.cards[1].source.symbol.range;
    session.cards[1].source.code_start = Some(Position::new(5, 0));
    session.cards[1].source.code = "    fn run() {}".into();
    session.cards[1]
        .source
        .context
        .push(refscape_model::SourceContext {
            start_line: 1,
            code: "impl Sample {".into(),
        });
    session.cards[1]
        .source
        .folded
        .push(refscape_model::SourceContext {
            start_line: 2,
            code: "    fn first() {}\n\n\n".into(),
        });
    session.connections.push(Connection {
        id: "main-to-run".into(),
        from: "main".into(),
        to: "run".into(),
        kind: ConnectionKind::Definition,
        source: Position::new(0, 3),
    });
    session.connections.push(Connection {
        id: "main-to-type".into(),
        from: "main".into(),
        to: "run".into(),
        kind: ConnectionKind::TypeDefinition,
        source: Position::new(0, 3),
    });
    session.regions.push(Region {
        id: "crate".into(),
        label: "Example crate".into(),
        path: directory.0.clone(),
        card_ids: vec!["main".into(), "run".into()],
    });
    session.viewport = Viewport {
        offset: Point::new(-220.0, 100.0),
        zoom: 0.75,
    };
    session.theme = Theme::light();
    // Version 1 snapshots with obsolete layout settings retain source and coordinates.
    let mut legacy = serde_json::to_value(&session).unwrap();
    legacy["layout_settings"] =
        serde_json::json!({"auto_compact_enabled":true,"auto_compact_min_reduction_percent":30});
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(JsonSessionRepository.load(&path).unwrap(), session);
    JsonSessionRepository.save(&path, &session).unwrap();
    assert_eq!(JsonSessionRepository.load(&path).unwrap(), session);
    let hidden = session.cards[1].source.folded.remove(0);
    session.cards[1].source.context[0].code.push('\n');
    session.cards[1].source.context[0]
        .code
        .push_str(&hidden.code);
    session.cards[1].source.expanded.push(hidden);
    JsonSessionRepository.save(&path, &session).unwrap();
    assert_eq!(JsonSessionRepository.load(&path).unwrap(), session);
    session.cards[0].position = Point::new(999.0, -222.0);
    JsonSessionRepository.save(&path, &session).unwrap();
    assert_eq!(JsonSessionRepository.load(&path).unwrap(), session);
}

#[test]
fn invalid_session_does_not_overwrite_last_valid_session() {
    let directory = TestDirectory::new();
    let path = directory.path("session.json");
    let session = Session::new(directory.0.clone());
    JsonSessionRepository.save(&path, &session).unwrap();
    let mut invalid = session.clone();
    invalid.viewport.zoom = f32::NAN;
    assert!(JsonSessionRepository.save(&path, &invalid).is_err());
    assert_eq!(JsonSessionRepository.load(&path).unwrap(), session);
}

#[test]
fn rejects_malformed_and_future_formats() {
    let directory = TestDirectory::new();
    let path = directory.path("session.json");
    fs::write(&path, "{not JSON").unwrap();
    assert!(JsonSessionRepository.load(&path).is_err());
    fs::write(&path, r#"{"version":999,"new_schema":true}"#).unwrap();
    let error = JsonSessionRepository.load(&path).unwrap_err();
    assert!(error.contains("unsupported session version 999"));
    fs::write(&path, r#"{"version":"1"}"#).unwrap();
    assert!(JsonSessionRepository.load(&path).is_err());
    fs::write(&path, r#"{"version":999}"#).unwrap();
    assert!(load_settings(&path).is_err());
    assert!(load_theme(&path).is_err());
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
    let path = directory.path("settings.json");
    let settings = Settings::default();
    save_settings(&path, &settings).unwrap();
    fs::write(directory.path("settings.json.123.456.tmp"), "{partial").unwrap();
    assert_eq!(load_settings(&path).unwrap(), settings);
    save_settings(&path, &settings).unwrap();
    assert_eq!(load_settings(&path).unwrap(), settings);
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
