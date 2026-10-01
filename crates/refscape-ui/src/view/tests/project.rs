use super::*;
type ProjectRequests = Arc<Mutex<Vec<(PathBuf, ProjectOptions)>>>;

struct ProjectLanguageFixture {
    requests: ProjectRequests,
    options: ProjectOptions,
    require_database: bool,
    files_error: bool,
    rejected_database: Option<PathBuf>,
}

impl LanguageService for ProjectLanguageFixture {
    fn open_project(&mut self, root: &Path, options: &ProjectOptions) -> Result<(), String> {
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
            ProjectOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.requests.error);
        assert!(view.session.project_root.as_os_str().is_empty());
        assert_eq!(view.project.pending.as_ref().unwrap().0, project.0);
    });
    view.update(cx, |view, cx| {
        view.select_compilation_database(database.clone(), cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.requests.error, "{}", view.requests.status);
        assert_eq!(view.session.project_root, project.0.canonicalize().unwrap());
        assert_eq!(
            view.session.project_options.compilation_database,
            Some(database.clone())
        );
        assert!(view.project.pending.is_none());
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
            ExplorerView::new(
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
            assert!(!view.requests.error, "{}", view.requests.status);
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
        explorer
            .open_project(&previous.0, &ProjectOptions::default())
            .unwrap();
        let old_session = previous.0.join("session.json");
        let (view, test_cx) = cx.add_window_view(|window, cx| {
            ExplorerView::new(
                explorer,
                old_session.clone(),
                vec![],
                None,
                ProjectOptions::default(),
                window,
                cx,
            )
        });
        test_cx.run_until_parked();
        view.update(test_cx, |view, cx| {
            view.project.files = vec![previous.0.join("old.cpp")];
            view.search.query = "old query".into();
            if open_saved_session {
                view.open_session(next_session.clone(), cx);
            } else {
                view.open_project_with_session(next.0.clone(), next_session.clone(), cx);
            }
        });
        test_cx.run_until_parked();
        view.read_with(test_cx, |view, _| {
            assert!(view.requests.error);
            assert!(view.requests.status.contains("source file listing failed"));
            assert_eq!(view.session.project_root, next.0.canonicalize().unwrap());
            assert_eq!(view.project.session_path, next_session);
            assert!(view.project.files.is_empty());
            assert!(view.search.query.is_empty());
            assert!(
                !view.project.autosave,
                "Enumeration failure must protect the saved session"
            );
            assert!(view.project.pending.is_none());
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
        ExplorerView::new(
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
    explorer
        .open_project(&previous.0, &ProjectOptions::default())
        .unwrap();
    let previous_path = previous.0.join("session.json");
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            previous_path.clone(),
            vec![],
            None,
            ProjectOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    view.update(cx, |view, cx| view.open_session(saved_path.clone(), cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.requests.error);
        assert_eq!(
            view.session.project_root,
            previous.0.canonicalize().unwrap()
        );
        assert_eq!(view.project.session_path, previous_path);
        assert_eq!(
            view.project.pending,
            Some((saved_project.0.canonicalize().unwrap(), saved_path.clone()))
        );
    });
    view.update(cx, |view, cx| {
        view.select_compilation_database(replacement.clone(), cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.requests.error, "{}", view.requests.status);
        assert_eq!(
            view.session.project_root,
            saved_project.0.canonicalize().unwrap()
        );
        assert_eq!(view.project.session_path, saved_path);
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
