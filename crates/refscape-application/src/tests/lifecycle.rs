use super::*;
#[test]
fn opening_and_restoring_preserve_analysis_options() {
    let mut h = Harness::new();
    let options = ProjectOpenOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(h.root.join("compile_commands.json")),
    };
    h.run(Command::OpenProject {
        root: h.root.clone(),
        options: options.clone(),
        destination: h.root.join("other.json"),
    });
    assert_eq!(h.snapshot().project_options, options);
    let snapshot = h.snapshot().clone();
    h.import(snapshot);
    assert_eq!(h.snapshot().project_options, options);
}
#[test]
fn restoring_checks_root_before_backend_startup() {
    let mut h = Harness::new();
    let before = h.snapshot().clone();
    let calls = h.state.lock().unwrap().prepare_calls;
    let other = h.root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    h.run(Command::OpenLoaded {
        loaded: ImportedSession {
            snapshot: before.clone(),
        },
        destination: h.root.join("new.json"),
        expected_root: Some(other),
        overrides: ProjectOpenOptions::default(),
    });
    assert_eq!(h.snapshot(), &before);
    assert_eq!(h.state.lock().unwrap().prepare_calls, calls);
}
#[test]
fn explicit_language_override_clears_saved_cpp_database() {
    for language in [
        ProjectLanguage::Rust,
        ProjectLanguage::TypeScript,
        ProjectLanguage::Python,
    ] {
        let mut h = Harness::new();
        let mut snapshot = h.snapshot().clone();
        snapshot.project_options = ProjectOpenOptions {
            language: ProjectLanguage::Cpp,
            compilation_database: Some(h.root.join("compile_commands.json")),
        };
        h.run(Command::OpenLoaded {
            loaded: ImportedSession { snapshot },
            destination: h.root.join("session.json"),
            expected_root: Some(h.root.clone()),
            overrides: ProjectOpenOptions {
                language,
                compilation_database: None,
            },
        });
        assert_eq!(h.snapshot().project_options.language, language);
        assert!(h.snapshot().project_options.compilation_database.is_none());
    }
}
#[test]
fn incompatible_override_is_rejected_before_backend_startup() {
    let mut h = Harness::new();
    let before = h.snapshot().clone();
    let calls = h.state.lock().unwrap().prepare_calls;
    h.run(Command::OpenLoaded {
        loaded: ImportedSession {
            snapshot: before.clone(),
        },
        destination: h.root.join("session.json"),
        expected_root: None,
        overrides: ProjectOpenOptions {
            language: ProjectLanguage::Python,
            compilation_database: Some("db.json".into()),
        },
    });
    assert_eq!(h.snapshot(), &before);
    assert_eq!(h.state.lock().unwrap().prepare_calls, calls);
}
#[test]
fn failed_candidate_retains_old_project_and_destination() {
    let mut h = Harness::new();
    h.add("a", Point::default());
    let before = h.snapshot().clone();
    let destination = h.driver.controller.destination().clone();
    h.state.lock().unwrap().fail_prepare = true;
    h.run(Command::OpenProject {
        root: h.root.clone(),
        options: ProjectOpenOptions::default(),
        destination: h.root.join("new.json"),
    });
    assert_eq!(h.snapshot(), &before);
    assert_eq!(h.driver.controller.destination(), &destination);
    assert_eq!(h.state.lock().unwrap().writes.len(), 1);
}
#[test]
fn listing_only_failure_adopts_project_and_protects_destination() {
    let mut h = Harness::new();
    h.add("a", Point::default());
    h.state.lock().unwrap().fail_list = true;
    let path = h.root.join("new.json");
    let events = h.run(Command::OpenProject {
        root: h.root.clone(),
        options: ProjectOpenOptions::default(),
        destination: path.clone(),
    });
    assert!(h.snapshot().cards.is_empty());
    assert!(
        matches!(h.driver.controller.destination(),SaveDestination::Protected {path:p,..} if p==&path)
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event,ViewEvent::Files(files) if files.is_empty()))
    );
    assert!(h.driver.controller.error());
}
#[test]
fn protected_close_never_overwrites_and_manual_save_recovers() {
    let mut h = Harness::new();
    h.state.lock().unwrap().fail_list = true;
    h.run(Command::OpenProject {
        root: h.root.clone(),
        options: ProjectOpenOptions::default(),
        destination: h.root.join("bad.json"),
    });
    let count = h.state.lock().unwrap().writes.len();
    let effects = h.pending(Command::RequestClose);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::CloseWindow))
    );
    assert_eq!(h.state.lock().unwrap().writes.len(), count);
    let mut h = Harness::new();
    h.state.lock().unwrap().fail_list = true;
    h.run(Command::OpenProject {
        root: h.root.clone(),
        options: ProjectOpenOptions::default(),
        destination: h.root.join("bad.json"),
    });
    h.run(Command::Save);
    assert!(matches!(
        h.driver.controller.destination(),
        SaveDestination::Writable(_)
    ));
}
#[test]
fn save_as_recovers_protection_after_success() {
    let mut h = Harness::new();
    h.state.lock().unwrap().fail_list = true;
    h.run(Command::OpenProject {
        root: h.root.clone(),
        options: ProjectOpenOptions::default(),
        destination: h.root.join("bad.json"),
    });
    let new = h.root.join("good.json");
    h.state.lock().unwrap().fail_save = true;
    h.run(Command::SaveAs(new.clone()));
    assert!(matches!(
        h.driver.controller.destination(),
        SaveDestination::Protected { .. }
    ));
    h.state.lock().unwrap().fail_save = false;
    h.run(Command::SaveAs(new.clone()));
    assert_eq!(
        h.driver.controller.destination(),
        &SaveDestination::Writable(new)
    );
}
#[test]
fn close_waits_for_save_and_failed_write_keeps_window_open() {
    let mut h = Harness::new();
    h.add("a", Point::default());
    let effects = h.pending(Command::RequestClose);
    assert!(h.driver.controller.closing());
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::CloseWindow))
    );
    h.state.lock().unwrap().fail_save = true;
    let effect = effects
        .into_iter()
        .find(|effect| matches!(effect, Effect::WriteSession { .. }))
        .unwrap();
    let result = h.execute(effect);
    let transition = h.finish(result);
    assert!(!h.driver.controller.closing());
    assert!(
        !transition
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::CloseWindow))
    );
    h.state.lock().unwrap().fail_save = false;
    let events = h.run(Command::RequestClose);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ViewEvent::CloseWindow))
    );
}
#[test]
fn session_loaded_once_is_reused_for_backend_failure_retry() {
    let mut h = Harness::new();
    let loaded = ImportedSession {
        snapshot: h.snapshot().clone(),
    };
    h.state.lock().unwrap().loaded = Some(loaded);
    h.state.lock().unwrap().fail_prepare = true;
    h.run(Command::OpenSession {
        path: h.root.join("loaded.json"),
        expected_root: None,
        overrides: ProjectOpenOptions::default(),
    });
    assert_eq!(h.state.lock().unwrap().load_calls, 1);
    assert_eq!(
        h.driver.controller.pending_project_root(),
        Some(h.root.canonicalize().unwrap().as_path())
    );
    h.state.lock().unwrap().fail_prepare = false;
    h.run(Command::SetCompilationDatabase(
        h.root.join("compile_commands.json"),
    ));
    assert_eq!(h.state.lock().unwrap().load_calls, 1);
    assert_eq!(h.snapshot().project_options.language, ProjectLanguage::Cpp);
}
#[test]
fn save_failure_prevents_candidate_preparation() {
    let mut h = Harness::new();
    let calls = h.state.lock().unwrap().prepare_calls;
    let before = h.snapshot().clone();
    h.state.lock().unwrap().fail_save = true;
    h.run(Command::OpenProject {
        root: h.root.clone(),
        options: ProjectOpenOptions::default(),
        destination: h.root.join("new.json"),
    });
    assert_eq!(h.state.lock().unwrap().prepare_calls, calls);
    assert_eq!(h.snapshot(), &before);
}
#[test]
fn legacy_missing_gap_only_fetches_on_first_expansion() {
    let mut h = Harness::new();
    let mut s = symbol("root");
    s.range.start.line = 3;
    s.range.end.line = 3;
    s.selection_range.start.line = 3;
    s.selection_range.end.line = 3;
    let mut doc = document(s.clone(), "fn target() {}");
    doc.context = vec![SourceContext {
        start_line: 0,
        code: "impl Project {".into(),
    }];
    h.state.lock().unwrap().sources.insert("root".into(), doc);
    h.run(Command::AddSymbol {
        symbol: s,
        position: Point::default(),
        toggle: false,
    });
    let id = h.card("root").id.to_string();
    let before = h.snapshot().clone();
    let calls = h.state.lock().unwrap().source_calls;
    h.state.lock().unwrap().fail_source = true;
    h.run(Command::ToggleFold {
        card: id.clone(),
        index: 0,
        expand: true,
    });
    assert_eq!(h.snapshot(), &before);
    h.state.lock().unwrap().fail_source = false;
    h.state.lock().unwrap().code =
        "impl Project {\n    fn first() {}\n\n    fn target() {}\n}".into();
    h.run(Command::ToggleFold {
        card: id.clone(),
        index: 0,
        expand: true,
    });
    assert_eq!(h.card("root").source.code.as_ref(), "fn target() {}");
    let fetched = h.state.lock().unwrap().source_calls;
    assert_eq!(fetched, calls + 2);
    h.run(Command::ToggleFold {
        card: id.clone(),
        index: 0,
        expand: false,
    });
    h.run(Command::ToggleFold {
        card: id,
        index: 0,
        expand: true,
    });
    assert_eq!(h.state.lock().unwrap().source_calls, fetched);
}
