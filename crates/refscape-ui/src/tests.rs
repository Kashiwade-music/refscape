use super::*;
use gpui::{Modifiers, TestAppContext, VisualContext};
use refscape_model::{SourceDocument, SourceRange};
use std::path::Path;

type ProjectRequests = Arc<Mutex<Vec<(PathBuf, ProjectOptions)>>>;

struct ProjectLanguageFixture {
    requests: ProjectRequests,
    options: ProjectOptions,
    require_database: bool,
    files_error: bool,
    rejected_database: Option<PathBuf>,
}

impl LanguageService for ProjectLanguageFixture {
    fn open_project(&mut self, root: &Path) -> Result<(), String> {
        self.open_project_with_options(root, &ProjectOptions::default())
    }
    fn open_project_with_options(
        &mut self,
        root: &Path,
        options: &ProjectOptions,
    ) -> Result<(), String> {
        self.requests
            .lock()
            .unwrap()
            .push((root.into(), options.clone()));
        if self.require_database
            && options.compilation_database.is_none()
            && self.requests.lock().unwrap().len() == 1
        {
            return Err("Multiple compilation databases; select Build settings.".into());
        }
        if self.rejected_database.is_some()
            && options.compilation_database == self.rejected_database
        {
            return Err("Saved compilation database is missing".into());
        }
        self.options = options.clone();
        Ok(())
    }
    fn project_options(&self) -> ProjectOptions {
        self.options.clone()
    }
    fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        if self.files_error {
            Err("source enumeration failed".into())
        } else {
            Ok(vec![])
        }
    }
    fn symbols(&mut self, _: &Path) -> Result<Vec<Symbol>, String> {
        Ok(vec![])
    }
    fn source(&mut self, _: &Symbol) -> Result<SourceDocument, String> {
        Err("No sources".into())
    }
    fn definitions(&mut self, _: &Path, _: Position) -> Result<Vec<Symbol>, String> {
        Ok(vec![])
    }
    fn references(&mut self, _: &Path, _: Position) -> Result<Vec<Symbol>, String> {
        Ok(vec![])
    }
}

struct ProjectRepositoryFixture(Option<Session>);
impl SessionRepository for ProjectRepositoryFixture {
    fn save(&self, _: &Path, _: &Session) -> Result<(), String> {
        Ok(())
    }
    fn load(&self, _: &Path) -> Result<Session, String> {
        self.0.clone().ok_or_else(|| "No saved session".into())
    }
}

struct TemporaryProject(PathBuf);
impl TemporaryProject {
    fn new() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("refscape-ui-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TemporaryProject {
    fn drop(&mut self) {
        if let (Ok(path), Ok(temp_root)) =
            (self.0.canonicalize(), std::env::temp_dir().canonicalize())
        {
            let expected_prefix = format!("refscape-ui-{}-", std::process::id());
            if path.parent() == Some(temp_root.as_path())
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(&expected_prefix))
            {
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
}

#[gpui::test]
fn compilation_database_retries_the_source_root_and_does_not_leak_to_next_project(
    cx: &mut TestAppContext,
) {
    let project = TemporaryProject::new();
    let next_project = TemporaryProject::new();
    let database = project.0.join("build/compile_commands.json");
    let requests = Arc::new(Mutex::new(vec![]));
    let explorer = Explorer::new(
        ProjectLanguageFixture {
            requests: requests.clone(),
            options: ProjectOptions::default(),
            require_database: true,
            files_error: false,
            rejected_database: None,
        },
        ProjectRepositoryFixture(None),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            PathBuf::new(),
            vec![],
            Some(project.0.clone()),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.error);
        assert!(view.session.project_root.as_os_str().is_empty());
        assert_eq!(view.pending_project.as_ref().unwrap().0, project.0);
    });
    view.update(cx, |view, cx| {
        view.select_compilation_database(database.clone(), cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.error, "{}", view.status);
        assert_eq!(view.session.project_root, project.0.canonicalize().unwrap());
        assert_eq!(
            view.session.project_options.compilation_database,
            Some(database.clone())
        );
        assert!(view.pending_project.is_none());
    });
    view.update(cx, |view, cx| {
        view.open_project_with_session(
            next_project.0.clone(),
            next_project.0.join("session.json"),
            cx,
        );
    });
    cx.run_until_parked();
    let requests = requests.lock().unwrap();
    assert_eq!(requests[1].0, project.0.canonicalize().unwrap());
    assert_eq!(requests[1].1.language, ProjectLanguage::Cpp);
    assert_eq!(requests[1].1.compilation_database, Some(database));
    assert_eq!(requests[2].0, next_project.0.canonicalize().unwrap());
    assert_eq!(requests[2].1, ProjectOptions::default());
}

#[gpui::test]
fn opening_restores_saved_build_settings_before_starting_and_accepts_explicit_override(
    cx: &mut TestAppContext,
) {
    let project = TemporaryProject::new();
    let session_path = project.0.join("session.json");
    std::fs::write(&session_path, "fixture").unwrap();
    let saved_database = project.0.join("build/debug/compile_commands.json");
    let override_database = project.0.join("build/release/compile_commands.json");
    let mut saved = Session::new(project.0.canonicalize().unwrap());
    saved.project_options = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(saved_database.clone()),
    };
    for (overrides, expected) in [
        (ProjectOptions::default(), saved_database),
        (
            ProjectOptions {
                language: ProjectLanguage::Cpp,
                compilation_database: Some(override_database.clone()),
            },
            override_database,
        ),
    ] {
        let requests = Arc::new(Mutex::new(vec![]));
        let explorer = Explorer::new(
            ProjectLanguageFixture {
                requests: requests.clone(),
                options: ProjectOptions::default(),
                require_database: true,
                files_error: false,
                rejected_database: None,
            },
            ProjectRepositoryFixture(Some(saved.clone())),
        );
        let (view, test_cx) = cx.add_window_view(|window, cx| {
            ExplorerView::new_with_options(
                explorer,
                session_path.clone(),
                vec![],
                Some(project.0.clone()),
                overrides,
                window,
                cx,
            )
        });
        test_cx.run_until_parked();
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            1,
            "Restoration should start the backend only once"
        );
        assert_eq!(requests[0].1.compilation_database, Some(expected.clone()));
        view.read_with(test_cx, |view, _| {
            assert!(!view.error, "{}", view.status);
            assert_eq!(
                view.session.project_options.compilation_database,
                Some(expected)
            );
        });
    }
}

#[test]
fn cpp_fallback_warning_remains_visible_in_project_settings() {
    let label = project_settings_label(&ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: None,
    });
    assert!(label.contains("References and symbol search may be incomplete"));
    assert!(label.contains("Build settings"));
}

struct RecordingProjectRepository {
    saved: Session,
    writes: Arc<Mutex<Vec<(PathBuf, PathBuf)>>>,
}
impl SessionRepository for RecordingProjectRepository {
    fn save(&self, path: &Path, session: &Session) -> Result<(), String> {
        self.writes
            .lock()
            .unwrap()
            .push((path.into(), session.project_root.clone()));
        Ok(())
    }
    fn load(&self, _: &Path) -> Result<Session, String> {
        Ok(self.saved.clone())
    }
}

#[gpui::test]
fn enumeration_failure_keeps_new_project_save_destination_for_project_and_session_open(
    cx: &mut TestAppContext,
) {
    let previous = TemporaryProject::new();
    let next = TemporaryProject::new();
    let next_session = next.0.join("session.json");
    std::fs::write(&next_session, "fixture").unwrap();
    for open_saved_session in [false, true] {
        let writes = Arc::new(Mutex::new(vec![]));
        let mut explorer = Explorer::new(
            ProjectLanguageFixture {
                requests: Arc::new(Mutex::new(vec![])),
                options: ProjectOptions::default(),
                require_database: false,
                files_error: true,
                rejected_database: None,
            },
            RecordingProjectRepository {
                saved: Session::new(next.0.canonicalize().unwrap()),
                writes: writes.clone(),
            },
        );
        explorer.open_project(&previous.0).unwrap();
        let old_session = previous.0.join("session.json");
        let (view, test_cx) = cx.add_window_view(|window, cx| {
            ExplorerView::new(explorer, old_session.clone(), vec![], None, window, cx)
        });
        test_cx.run_until_parked();
        view.update(test_cx, |view, cx| {
            view.files = vec![previous.0.join("old.cpp")];
            view.query = "old query".into();
            if open_saved_session {
                view.open_session(next_session.clone(), cx);
            } else {
                view.open_project_with_session(next.0.clone(), next_session.clone(), cx);
            }
        });
        test_cx.run_until_parked();
        view.read_with(test_cx, |view, _| {
            assert!(view.error);
            assert!(view.status.contains("source file listing failed"));
            assert_eq!(view.session.project_root, next.0.canonicalize().unwrap());
            assert_eq!(view.session_path, next_session);
            assert!(view.files.is_empty());
            assert!(view.query.is_empty());
            assert!(
                !view.autosave,
                "Enumeration failure must protect the saved session"
            );
            assert!(view.pending_project.is_none());
        });
        view.update(test_cx, |view, cx| view.save(cx));
        test_cx.run_until_parked();
        assert_eq!(
            writes.lock().unwrap().last().unwrap(),
            &(next_session.clone(), next.0.canonicalize().unwrap())
        );
        assert!(!writes.lock().unwrap().iter().any(|(path, root)| path == &old_session && root == &next.0.canonicalize().unwrap()));
    }
}

#[gpui::test]
fn launch_settings_survive_first_folder_picker_and_clear_after_opening(cx: &mut TestAppContext) {
    let project = TemporaryProject::new();
    let next = TemporaryProject::new();
    let requests = Arc::new(Mutex::new(vec![]));
    let options = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(project.0.join("build/compile_commands.json")),
    };
    let explorer = Explorer::new(
        ProjectLanguageFixture {
            requests: requests.clone(),
            options: ProjectOptions::default(),
            require_database: false,
            files_error: false,
            rejected_database: None,
        },
        ProjectRepositoryFixture(None),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new_with_options(
            explorer,
            PathBuf::new(),
            vec![],
            None,
            options.clone(),
            window,
            cx,
        )
    });
    view.update(cx, |view, cx| {
        view.open_project_with_session(project.0.clone(), project.0.join("session.json"), cx)
    });
    cx.run_until_parked();
    assert_eq!(requests.lock().unwrap()[0].1, options);
    view.update(cx, |view, cx| {
        view.open_project_with_session(next.0.clone(), next.0.join("session.json"), cx)
    });
    cx.run_until_parked();
    assert_eq!(requests.lock().unwrap()[1].1, ProjectOptions::default());
}

#[gpui::test]
fn failed_session_startup_build_settings_retry_targets_saved_source_root(cx: &mut TestAppContext) {
    let previous = TemporaryProject::new();
    let saved_project = TemporaryProject::new();
    let missing_database = saved_project.0.join("missing/compile_commands.json");
    let replacement = saved_project.0.join("build/compile_commands.json");
    let saved_path = saved_project.0.join("session.json");
    std::fs::write(&saved_path, "fixture").unwrap();
    let mut saved = Session::new(saved_project.0.canonicalize().unwrap());
    saved.project_options = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(missing_database.clone()),
    };
    let requests = Arc::new(Mutex::new(vec![]));
    let mut explorer = Explorer::new(
        ProjectLanguageFixture {
            requests: requests.clone(),
            options: ProjectOptions::default(),
            require_database: false,
            files_error: false,
            rejected_database: Some(missing_database),
        },
        ProjectRepositoryFixture(Some(saved)),
    );
    explorer.open_project(&previous.0).unwrap();
    let previous_path = previous.0.join("session.json");
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(explorer, previous_path.clone(), vec![], None, window, cx)
    });
    cx.run_until_parked();
    view.update(cx, |view, cx| view.open_session(saved_path.clone(), cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.error);
        assert_eq!(
            view.session.project_root,
            previous.0.canonicalize().unwrap()
        );
        assert_eq!(view.session_path, previous_path);
        assert_eq!(
            view.pending_project,
            Some((saved_project.0.canonicalize().unwrap(), saved_path.clone()))
        );
    });
    view.update(cx, |view, cx| {
        view.select_compilation_database(replacement.clone(), cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.error, "{}", view.status);
        assert_eq!(
            view.session.project_root,
            saved_project.0.canonicalize().unwrap()
        );
        assert_eq!(view.session_path, saved_path);
        assert_eq!(
            view.session.project_options.compilation_database,
            Some(replacement)
        );
    });
    assert_eq!(
        requests.lock().unwrap()[2].0,
        saved_project.0.canonicalize().unwrap()
    );
}

type Requests = Arc<Mutex<Vec<Position>>>;

struct Language {
    source: SourceDocument,
    requests: Arc<Mutex<Vec<Position>>>,
    targets: Vec<Symbol>,
    type_targets: Vec<Symbol>,
    type_requests: Arc<Mutex<Vec<Position>>>,
    highlights: Vec<SourceRange>,
}
impl LanguageService for Language {
    fn open_project(&mut self, _: &Path) -> Result<(), String> {
        Ok(())
    }
    fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        Ok(vec![])
    }
    fn symbols(&mut self, _: &Path) -> Result<Vec<Symbol>, String> {
        Ok(vec![])
    }
    fn source(&mut self, symbol: &Symbol) -> Result<SourceDocument, String> {
        let mut source = self.source.clone();
        source.symbol = symbol.clone();
        Ok(source)
    }
    fn definitions(&mut self, _: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.requests.lock().unwrap().push(position);
        Ok(self.targets.clone())
    }
    fn references(&mut self, _: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.requests.lock().unwrap().push(position);
        Ok(self.targets.clone())
    }
    fn type_definitions(&mut self, _: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.type_requests.lock().unwrap().push(position);
        Ok(self.type_targets.clone())
    }
    fn document_highlights(&mut self, _: &Path, _: Position) -> Result<Vec<SourceRange>, String> {
        Ok(self.highlights.clone())
    }
    fn hover(&mut self, _: &Path, position: Position) -> Result<Option<String>, String> {
        self.requests.lock().unwrap().push(position);
        if self.source.variable_token(position).is_some() {
            return Ok(Some(format!(
                "let call: {}",
                if self.type_targets.is_empty() {
                    "u32"
                } else {
                    "Config"
                }
            )));
        }
        Ok((position == Position::new(12, 9))
            .then(|| "fn call() -> u32\n\nCalls the helper.".into()))
    }
    fn search(&mut self, _: &str) -> Result<Vec<Symbol>, String> {
        Err("simulated analyzer failure".into())
    }
}
struct Repository;
impl SessionRepository for Repository {
    fn save(&self, _: &Path, _: &Session) -> Result<(), String> {
        Err("simulated storage failure".into())
    }
    fn load(&self, _: &Path) -> Result<Session, String> {
        Err("invalid fixture session".into())
    }
}
fn fixture() -> (Explorer<Language, Repository>, Arc<Mutex<Vec<Position>>>) {
    fixture_with_targets(vec![])
}
fn fixture_with_targets(
    targets: Vec<Symbol>,
) -> (Explorer<Language, Repository>, Arc<Mutex<Vec<Position>>>) {
    let range = SourceRange {
        start: Position::new(12, 5),
        end: Position::new(12, 13),
    };
    let symbol = Symbol::file(PathBuf::from("sample.rs"), range);
    let source = SourceDocument {
        symbol: symbol.clone(),
        code: "日本😀call".into(),
        tokens: vec![],
    };
    let requests = Arc::new(Mutex::new(vec![]));
    let mut explorer = Explorer::new(
        Language {
            source,
            requests: requests.clone(),
            targets,
            type_targets: vec![],
            type_requests: Arc::new(Mutex::new(vec![])),
            highlights: vec![],
        },
        Repository,
    );
    explorer
        .add_symbol(symbol, Point::new(100.0, 50.0))
        .unwrap();
    (explorer, requests)
}

fn variable_fixture(has_type: bool) -> (Explorer<Language, Repository>, Requests, Requests) {
    let range = SourceRange {
        start: Position::new(12, 5),
        end: Position::new(13, 8),
    };
    let symbol = Symbol::file("sample.rs".into(), range);
    let mut definition = symbol.clone();
    definition.id = "binding".into();
    definition.name = "binding".into();
    definition.kind = "location".into();
    let mut type_symbol = Symbol::file("type.rs".into(), range);
    type_symbol.name = "Config".into();
    let source = SourceDocument {
        symbol: symbol.clone(),
        code: "日本😀call call\n    call".into(),
        tokens: [(12, 9), (12, 14), (13, 4)]
            .into_iter()
            .map(|(line, start)| refscape_model::SemanticToken {
                line,
                start,
                length: 4,
                kind: "variable".into(),
                modifiers: vec![],
            })
            .collect(),
    };
    let requests = Arc::new(Mutex::new(vec![]));
    let type_requests = Arc::new(Mutex::new(vec![]));
    let mut explorer = Explorer::new(
        Language {
            source,
            requests: requests.clone(),
            targets: vec![definition],
            type_targets: if has_type { vec![type_symbol] } else { vec![] },
            type_requests: type_requests.clone(),
            highlights: vec![
                SourceRange {
                    start: Position::new(12, 9),
                    end: Position::new(12, 13),
                },
                SourceRange {
                    start: Position::new(13, 4),
                    end: Position::new(13, 8),
                },
            ],
        },
        Repository,
    );
    explorer
        .add_symbol(symbol, Point::new(100.0, 50.0))
        .unwrap();
    (explorer, requests, type_requests)
}

#[gpui::test]
fn variable_click_highlights_identity_and_toggles_type_at_any_glyph(cx: &mut TestAppContext) {
    let (explorer, requests, type_requests) = variable_fixture(true);
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(explorer, "session.json".into(), vec![], None, window, cx)
    });
    let handle = cx.window_handle();
    for (glyph, count) in [("日本😀ca", 2), ("日本😀c", 1), ("日本😀", 2)] {
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let click = view.read_with(cx, |view, _| {
            let source = &view.painted[0];
            point(
                source.origin.x + source.lines[0].x_for_index(glyph.len()) + px(1.0),
                source.origin.y + px(5.0),
            )
        });
        cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(!view.error, "{}", view.status);
            assert_eq!(view.session.cards.len(), count);
            let origin = &view.session.cards[0];
            assert_eq!(
                variable_highlight_spans(origin, 0, view.inspection.as_ref()),
                vec!["日本😀".len().."日本😀call".len()]
            );
            assert_eq!(
                variable_highlight_spans(origin, 1, view.inspection.as_ref()),
                vec![4..8]
            );
            if count == 2 {
                assert_eq!(
                    view.session.connections[0].kind,
                    ConnectionKind::TypeDefinition
                );
                assert_eq!(view.session.connections[0].source, Position::new(12, 9));
                assert_eq!(
                    card_title(&view.session, &view.session.cards[1]),
                    "call → Config"
                );
                assert!(
                    variable_highlight_spans(&view.session.cards[1], 0, view.inspection.as_ref())
                        .is_empty()
                );
            }
        });
    }
    assert_eq!(
        *type_requests.lock().unwrap(),
        vec![Position::new(12, 9); 2]
    );
    // Only hover asks the ordinary request recorder; no binding-definition navigation.
    assert_eq!(*requests.lock().unwrap(), vec![Position::new(12, 9); 3]);
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |view, _| assert!(view.inspection.is_none()));
}

#[gpui::test]
fn primitive_variable_keeps_highlights_and_alt_click_opens_binding(cx: &mut TestAppContext) {
    let (explorer, requests, type_requests) = variable_fixture(false);
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(explorer, "session.json".into(), vec![], None, window, cx)
    });
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let click = view.read_with(cx, |view, _| {
        let source = &view.painted[0];
        point(
            source.origin.x + source.lines[0].x_for_index("日本😀".len()) + px(1.0),
            source.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards.len(), 1);
        assert!(view.inspection.is_some());
        assert_eq!(
            view.inspection.as_ref().unwrap().description.as_deref(),
            Some("let call: u32")
        );
        assert!(!view.error);
    });
    assert_eq!(type_requests.lock().unwrap().len(), 1);
    cx.simulate_mouse_down(
        click,
        MouseButton::Left,
        Modifiers {
            alt: true,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.inspection.is_none());
        assert_eq!(view.session.cards.len(), 2);
        assert_eq!(view.session.connections[0].kind, ConnectionKind::Definition);
    });
    assert_eq!(*requests.lock().unwrap(), vec![Position::new(12, 9); 2]);
}

#[gpui::test]
fn clearing_selection_during_analysis_does_not_restore_stale_highlights(cx: &mut TestAppContext) {
    let (explorer, _, _) = variable_fixture(false);
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(explorer, "session.json".into(), vec![], None, window, cx)
    });
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let click = view.read_with(cx, |view, _| {
        let source = &view.painted[0];
        point(
            source.origin.x + source.lines[0].x_for_index("日本😀".len()) + px(1.0),
            source.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.busy);
        assert!(!view.error);
        assert!(view.inspection.is_none());
    });
}

#[gpui::test]
fn source_hover_is_debounced_and_uses_absolute_utf16_at_each_zoom(cx: &mut TestAppContext) {
    let (explorer, requests) = fixture();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(explorer, "session.json".into(), vec![], None, window, cx)
    });
    let handle = cx.window_handle();
    for (zoom, offset) in [
        (1.0, Point::default()),
        (0.75, Point::new(30.0, 45.0)),
        (1.5, Point::new(-40.0, 10.0)),
    ] {
        view.update(cx, |view, cx| {
            view.clear_hover(cx);
            view.session.viewport.zoom = zoom;
            view.session.viewport.offset = offset;
            cx.notify();
        });
        requests.lock().unwrap().clear();
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let (word, other_glyph, header, empty) = view.read_with(cx, |view, _| {
            let card = &view.painted[0];
            (
                point(
                    card.origin.x + card.lines[0].x_for_index("日本😀".len()) + px(1.0),
                    card.origin.y + px(5.0 * zoom),
                ),
                point(
                    card.origin.x + card.lines[0].x_for_index("日本😀ca".len()) + px(1.0),
                    card.origin.y + px(5.0 * zoom),
                ),
                point(card.bounds.left() + px(20.0), card.bounds.top() + px(10.0)),
                point(
                    view.bounds.right() - px(10.0),
                    view.bounds.bottom() - px(10.0),
                ),
            )
        });
        cx.simulate_mouse_move(word, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
        assert!(requests.lock().unwrap().is_empty());
        cx.simulate_mouse_move(header, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        assert!(
            requests.lock().unwrap().is_empty(),
            "leaving before the delay cancels the request"
        );
        cx.simulate_mouse_move(word, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        assert_eq!(*requests.lock().unwrap(), vec![Position::new(12, 9)]);
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.hover_text.as_deref(),
                Some("fn call() -> u32\n\nCalls the helper.")
            );
            assert!(!view.busy);
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        cx.simulate_mouse_move(other_glyph, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        assert_eq!(
            requests.lock().unwrap().len(),
            1,
            "moving inside the same word reuses its hover"
        );
        let panel = cx.debug_bounds("code-hover").expect("rendered hover panel");
        view.read_with(cx, |view, _| {
            assert!(panel.left() >= view.bounds.left());
            assert!(panel.right() <= view.bounds.right());
            assert!(panel.top() >= view.bounds.top());
            assert!(panel.bottom() <= view.bounds.bottom());
        });
        let inside_panel = point(panel.left() + px(20.0), panel.top() + px(20.0));
        let gap = point(word.x, panel.top() - px(3.0));
        cx.simulate_mouse_move(gap, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(
                view.hover_text.is_some(),
                "crossing the popup gap keeps it open"
            );
        });
        cx.simulate_mouse_move(inside_panel, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(
                view.hover_text.is_some(),
                "the popup remains readable while hovered"
            )
        });
        cx.simulate_click(inside_panel, Modifiers::default());
        assert_eq!(
            requests.lock().unwrap().len(),
            1,
            "clicks inside the popup must not open definitions"
        );
        cx.simulate_mouse_move(empty, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        view.read_with(cx, |view, _| assert!(view.hover_text.is_none()));
    }
}

#[gpui::test]
fn long_hover_documentation_scrolls_without_moving_the_canvas(cx: &mut TestAppContext) {
    let (explorer, requests) = fixture();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(explorer, "session.json".into(), vec![], None, window, cx)
    });
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let word = view.read_with(cx, |view, _| {
        let card = &view.painted[0];
        point(
            card.origin.x + card.lines[0].x_for_index("日本😀".len()) + px(1.0),
            card.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_move(word, None, Modifiers::default());
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        // A real pointer path can pass over a word on the next source row.
        view.session.cards[0].source.code.push_str("\n日本😀other");
        view.session.cards[0].source.symbol.range.end = Position::new(13, 9);
        view.hover_text = Some(
            (0..80)
                .map(|line| format!("Documentation line {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let panel = cx.debug_bounds("code-hover").unwrap();
    let inside = point(panel.left() + px(20.0), panel.top() + px(20.0));
    cx.simulate_mouse_move(
        point(word.x, panel.top() - px(3.0)),
        None,
        Modifiers::default(),
    );
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.hover_text.is_some()));
    cx.simulate_mouse_move(inside, None, Modifiers::default());
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    let viewport = view.read_with(cx, |view, _| view.session.viewport);
    cx.simulate_event(ScrollWheelEvent {
        position: inside,
        delta: ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
        ..Default::default()
    });
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    view.read_with(cx, |view, _| {
        assert!(view.hover_text.is_some());
        assert!(
            view.hover_scroll.offset().y < px(0.0),
            "documentation actually scrolls"
        );
        assert_eq!(view.session.viewport, viewport);
        assert_eq!(requests.lock().unwrap().len(), 1);
    });
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |view, _| {
        assert!(view.hover_text.is_none());
        assert_eq!(view.hover_scroll.offset().y, px(0.0));
    });
}

#[gpui::test]
fn hover_cancels_when_zooming_and_never_requests_hidden_source(cx: &mut TestAppContext) {
    let (explorer, requests) = fixture();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(explorer, "session.json".into(), vec![], None, window, cx)
    });
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let word = view.read_with(cx, |view, _| {
        let card = &view.painted[0];
        point(
            card.origin.x + card.lines[0].x_for_index("日本😀".len()) + px(1.0),
            card.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_move(word, None, Modifiers::default());
    view.update(cx, |view, cx| view.zoom(0.5, Point::default(), cx));
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    cx.simulate_mouse_move(word, None, Modifiers::default());
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    assert!(requests.lock().unwrap().is_empty());
    view.read_with(cx, |view, _| assert!(view.hover_target.is_none()));
}

#[gpui::test]
fn clicking_a_linked_word_toggles_cards_even_at_different_glyphs(cx: &mut TestAppContext) {
    let mut target = Symbol::file(
        "target.rs".into(),
        SourceRange {
            start: Position::new(12, 5),
            end: Position::new(12, 13),
        },
    );
    target.id = "target".into();
    let (explorer, requests) = fixture_with_targets(vec![target]);
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(explorer, "session.json".into(), vec![], None, window, cx)
    });
    let handle = cx.window_handle();
    for (glyph, count) in [("日本😀", 2), ("日本😀ca", 1), ("日本😀c", 2)] {
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let click = view.read_with(cx, |view, _| {
            let source = &view.painted[0];
            point(
                source.origin.x + source.lines[0].x_for_index(glyph.len()) + px(1.0),
                source.origin.y + px(5.0),
            )
        });
        cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(!view.error, "{}", view.status);
            assert_eq!(view.session.cards.len(), count);
            assert_eq!(view.session.connections.len(), count - 1);
        });
    }
    // Hiding an existing link is local and should not ask the analyzer again.
    assert_eq!(requests.lock().unwrap().len(), 2);
    let picker_symbol = view.read_with(cx, |view, _| view.session.cards[1].source.symbol.clone());
    view.update(cx, |view, cx| {
        view.selected = Some(view.session.cards[1].id.clone());
        view.toggle_symbol(picker_symbol.clone(), cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards.len(), 1);
        assert!(view.session.connections.is_empty());
        assert!(view.selected.is_none());
    });
    view.update(cx, |view, cx| view.toggle_symbol(picker_symbol, cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.session.cards.len(), 2));
}

#[gpui::test]
fn hiding_a_card_applies_the_new_layout_to_the_canvas_and_connection_anchors(
    cx: &mut TestAppContext,
) {
    let range = SourceRange {
        start: Position::new(12, 5),
        end: Position::new(12, 13),
    };
    let first = Symbol::file("first.rs".into(), range);
    let second = Symbol::file("second.rs".into(), range);
    let (mut explorer, _) = fixture_with_targets(vec![first.clone(), second.clone()]);
    let far = explorer
        .add_symbol(
            Symbol::file("far.rs".into(), range),
            Point::new(1960.0, 50.0),
        )
        .unwrap();
    explorer.zoom(1.25, Point::default()).unwrap();
    explorer.pan(Point::new(-50.0, 20.0)).unwrap();
    let viewport = explorer.session().viewport;
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(explorer, "session.json".into(), vec![], None, window, cx)
    });
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let click = view.read_with(cx, |view, _| {
        let source = &view.painted[0];
        point(
            source.origin.x + source.lines[0].x_for_index("日本😀".len()) + px(1.0),
            source.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    view.update(cx, |view, cx| view.toggle_symbol(first, cx));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let (session, bounds) = view.read_with(cx, |view, _| {
        assert!(!view.error, "{}", view.status);
        assert_eq!(view.session.cards.len(), 3);
        let target = view
            .session
            .cards
            .iter()
            .find(|card| card.source.symbol.path == second.path)
            .unwrap();
        assert_eq!(target.position, Point::new(720.0, 50.0 + HEADER + 8.0));
        assert_eq!(
            view.session
                .cards
                .iter()
                .find(|card| card.id == far)
                .unwrap()
                .position,
            Point::new(1340.0, 50.0)
        );
        assert_eq!(view.session.viewport, viewport);
        assert_eq!(
            view.session.cards,
            view.explorer.lock().unwrap().session().cards
        );
        (view.session.clone(), view.bounds)
    });
    cx.update_window(handle, |_, window, _| {
        let links = code_connections(&session, bounds, window);
        assert_eq!(links.len(), 1);
        let target = session
            .cards
            .iter()
            .find(|card| card.source.symbol.path == second.path)
            .unwrap();
        let target_bounds = card_bounds(target, &session, bounds);
        assert_eq!(
            links[0].end,
            point(
                target_bounds.left(),
                target_bounds.top() + px(HEADER * viewport.zoom * 0.5)
            )
        );
    })
    .unwrap();
}

#[gpui::test]
fn native_source_click_uses_shaped_glyphs_and_absolute_utf16_positions(cx: &mut TestAppContext) {
    let (explorer, requests) = fixture();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            PathBuf::from("session.json"),
            vec![],
            None,
            window,
            cx,
        )
    });
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let click = view.read_with(cx, |view, _| {
        let card = &view.painted[0];
        point(
            card.origin.x + card.lines[0].x_for_index("日本😀".len()) + px(1.0),
            card.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(*requests.lock().unwrap(), vec![Position::new(12, 9)]);
    cx.simulate_mouse_down(click, MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[gpui::test]
fn asynchronous_failure_keeps_canvas_movement_and_pointer_zoom_anchor(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            PathBuf::from("session.json"),
            vec![],
            None,
            window,
            cx,
        )
    });
    view.update(cx, |view, cx| {
        view.search(cx);
        let anchor = Point::new(400.0, 200.0);
        let before = view.session.viewport.screen_to_world(anchor);
        view.zoom(1.5, anchor, cx);
        assert_eq!(before, view.session.viewport.screen_to_world(anchor));
        view.drag = Some(Drag::Pan(point(px(0.0), px(0.0))));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(35.0), px(60.0)),
                ..Default::default()
            },
            cx,
        );
        view.drag = Some(Drag::Card(
            view.session.cards[0].id.clone(),
            point(px(0.0), px(0.0)),
            Point::new(100.0, 50.0),
        ));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(150.0), px(75.0)),
                ..Default::default()
            },
            cx,
        );
    });
    let moved = view.read_with(cx, |view, _| {
        (view.session.viewport, view.session.cards[0].position)
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.error);
        assert_eq!(view.status, "simulated analyzer failure");
        assert_eq!(
            (view.session.viewport, view.session.cards[0].position),
            moved
        );
    });
}

#[gpui::test]
fn portable_custom_theme_survives_cycle_and_close_waits_for_requests(cx: &mut TestAppContext) {
    let (mut explorer, _) = fixture();
    let mut custom = Theme::dark();
    custom.palette.accent = "#FF0000".into();
    explorer.set_theme(custom.clone()).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            PathBuf::from("session.json"),
            vec![Theme::dark(), custom.clone()],
            None,
            window,
            cx,
        )
    });
    view.update_in(cx, |view, window, cx| {
        assert_eq!(view.themes.len(), 3);
        view.busy = true;
        assert!(!view.close(window, cx));
        assert!(!view.closing);
        view.busy = false;
        view.cycle_theme(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.session.theme, Theme::dark()));
    view.update(cx, |view, cx| view.cycle_theme(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.session.theme, Theme::light()));
    view.update(cx, |view, cx| view.cycle_theme(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.session.theme, custom));
}

#[gpui::test]
fn definition_and_reference_edges_start_at_rendered_word_underlines(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
    let mut session = explorer.session().clone();
    session.cards[0]
        .source
        .tokens
        .push(refscape_model::SemanticToken {
            line: 12,
            start: 9,
            length: 4,
            kind: "function".into(),
            modifiers: vec![],
        });
    let mut target = session.cards[0].clone();
    target.id = "target".into();
    target.position = Point::new(800.0, 100.0);
    session.cards.push(target);
    for (index, kind) in [
        refscape_model::ConnectionKind::Definition,
        refscape_model::ConnectionKind::Reference,
    ]
    .into_iter()
    .enumerate()
    {
        session.connections.push(refscape_model::Connection {
            id: format!("edge-{index}"),
            from: session.cards[0].id.clone(),
            to: "target".into(),
            kind,
            source: Position::new(12, 11),
        });
    }
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            PathBuf::from("session.json"),
            vec![],
            None,
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let handle = cx.window_handle();
    for (zoom, offset) in [
        (1.0, Point::default()),
        (0.75, Point::new(30.0, 45.0)),
        (1.5, Point::new(-40.0, 10.0)),
    ] {
        session.viewport.zoom = zoom;
        session.viewport.offset = offset;
        view.update(cx, |view, cx| {
            view.session = session.clone();
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let (bounds, word_start, word_end) = view.read_with(cx, |view, _| {
            let card = &view.painted[0];
            (
                view.bounds,
                card.origin.x + card.lines[0].x_for_index("日本😀".len()),
                card.origin.x + card.lines[0].x_for_index("日本😀call".len()),
            )
        });
        cx.update_window(handle, |_, window, _| {
            let edges = code_connections(&session, bounds, window);
            assert_eq!(edges.len(), 2);
            for edge in edges {
                assert!(f32::from(edge.underline.left() - word_start).abs() < 0.001);
                assert!(f32::from(edge.underline.right() - word_end).abs() < 0.001);
                assert!(f32::from(edge.start.x - word_end).abs() < 0.001);
                assert!(edge.start.x < card_bounds(&session.cards[0], &session, bounds).right());
                let painted_bounds = edge.underline.scale(window.scale_factor());
                assert!(
                    window.painted_quads().iter().any(|quad| {
                        // GPUI snaps filled rectangles to physical pixel edges.
                        (quad.bounds.left().0 - painted_bounds.left().0).abs() <= 0.51
                            && (quad.bounds.right().0 - painted_bounds.right().0).abs() <= 0.51
                            && (quad.bounds.top().0 - painted_bounds.top().0).abs() <= 0.51
                            && (quad.bounds.bottom().0 - painted_bounds.bottom().0).abs() <= 0.51
                    }),
                    "the linked word must actually be underlined in the rendered scene"
                );
            }
        })
        .unwrap();
    }
    // A restored snapshot without semantic tokens still attaches to text, and
    // abstract zoom levels never fall back to a card-edge attachment.
    session.cards[0].source.tokens.clear();
    assert_eq!(
        connected_word(&session.cards[0], Position::new(12, 11))
            .unwrap()
            .1,
        "日本😀".len().."日本😀call".len()
    );
    session.viewport.zoom = 0.5;
    cx.update_window(handle, |_, window, _| {
        assert!(code_connections(&session, Bounds::default(), window).is_empty());
    })
    .unwrap();
}

#[gpui::test]
fn dropping_tall_cards_clears_their_rendered_bottoms_at_every_zoom(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            PathBuf::from("session.json"),
            vec![],
            None,
            window,
            cx,
        )
    });
    let handle = cx.window_handle();
    for zoom in [0.75, 1.0, 1.5] {
        view.update(cx, |view, cx| {
            let mut first = view.session.cards[0].clone();
            first.source.code = std::iter::repeat_n("fn source() {}", 12)
                .collect::<Vec<_>>()
                .join("\n");
            first.height = 128.0;
            first.position = Point::new(20.0, 10.0);
            let mut second = first.clone();
            second.id = "second".into();
            second.position.y = 170.0;
            let mut third = first.clone();
            third.id = "third".into();
            third.position.y = 330.0;
            view.session.cards = vec![first, second, third];
            view.session.viewport.zoom = zoom;
            view.session.viewport.offset = Point::new(13.0, 27.0);
            view.drag = Some(Drag::Card(
                "second".into(),
                point(px(0.0), px(0.0)),
                Point::new(20.0, 170.0),
            ));
            view.finish_drag(cx);
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        view.read_with(cx, |view, _| {
            assert!(view.drag.is_none());
            let rects: Vec<_> = view
                .session
                .cards
                .iter()
                .map(|card| card_bounds(card, &view.session, view.bounds))
                .collect();
            for pair in rects.windows(2) {
                assert!(f32::from(pair[1].top() - pair[0].bottom()) >= 32.0 * zoom - 0.001);
            }
            for painted in &view.painted {
                let last_line_bottom =
                    painted.origin.y + px(painted.lines.len() as f32 * LINE * zoom);
                assert!(painted.bounds.bottom() >= last_line_bottom + px(16.0 * zoom));
                let card = view
                    .session
                    .cards
                    .iter()
                    .find(|card| card.id == painted.id)
                    .unwrap();
                assert_eq!(painted.bounds.size.height, px(card.height * zoom));
            }
        });
    }
}

#[gpui::test]
fn cards_moved_during_a_request_do_not_overlap_new_cards_on_completion(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
    let mut target = explorer.session().cards[0].source.symbol.clone();
    target.id = "new-target".into();
    target.path = "target.rs".into();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(explorer, "session.json".into(), vec![], None, window, cx)
    });
    view.update(cx, |view, cx| {
        view.run_job(
            "Opening target",
            Box::new(move |explorer| {
                explorer.add_symbol(target, Point::new(800.0, 50.0))?;
                Ok(Output::default())
            }),
            cx,
        );
        // The worker planned against the old position before the pointer moved.
        view.drag = Some(Drag::Card(
            view.session.cards[0].id.clone(),
            point(px(0.0), px(0.0)),
            view.session.cards[0].position,
        ));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(700.0), px(0.0)),
                ..Default::default()
            },
            cx,
        );
        view.finish_drag(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.error);
        assert_eq!(view.session.cards.len(), 2);
        assert_eq!(view.session.cards[0].position, Point::new(800.0, 50.0));
        let source = card_bounds(&view.session.cards[0], &view.session, view.bounds);
        let target = card_bounds(&view.session.cards[1], &view.session, view.bounds);
        assert!(f32::from(target.top() - source.bottom()) >= 32.0);
    });
    // A failing request also merges the current UI positions, then clears overlap.
    view.update(cx, |view, cx| {
        view.search(cx);
        view.session.cards[0].position = view.session.cards[1].position;
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.error);
        let source = card_bounds(&view.session.cards[0], &view.session, view.bounds);
        let target = card_bounds(&view.session.cards[1], &view.session, view.bounds);
        assert!(f32::from(target.top() - source.bottom()) >= 32.0);
    });
}
