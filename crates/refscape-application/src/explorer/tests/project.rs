use super::*;

#[test]
fn opening_and_restoring_preserve_source_root_and_analysis_options() {
    let root = std::env::current_dir().unwrap();
    let options = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(root.join("out/debug/compile_commands.json")),
    };
    let mut original = explorer();
    original.open_project(&root, &options).unwrap();
    assert_eq!(original.session.project_root, root.canonicalize().unwrap());
    assert_eq!(original.language.options, options);
    assert_eq!(original.session.project_options, options);

    struct Saved(Session);
    impl SessionRepository for Saved {
        fn save(&self, _: &Path, _: &Session) -> Result<()> {
            Ok(())
        }
        fn load(&self, _: &Path) -> Result<Session> {
            Ok(self.0.clone())
        }
    }
    let saved = original.session.clone();
    original.language.options = ProjectOptions::default();
    let mut restored = Explorer::new(original.language, Saved(saved.clone()));
    restored.load_session(Path::new("session.json")).unwrap();
    assert_eq!(restored.language.options, options);
    assert_eq!(restored.session, saved);

    restored
        .open_project(&root, &ProjectOptions::default())
        .unwrap();
    assert_eq!(restored.session.project_options, ProjectOptions::default());
}

#[test]
fn restoring_project_checks_root_and_merges_overrides_before_backend_startup() {
    struct Saved(Session);
    impl SessionRepository for Saved {
        fn save(&self, _: &Path, _: &Session) -> Result<()> {
            Ok(())
        }
        fn load(&self, _: &Path) -> Result<Session> {
            Ok(self.0.clone())
        }
    }
    let original = explorer();
    let root = original.session.project_root.clone();
    let mut saved = original.session;
    saved.project_options = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(root.join("saved/compile_commands.json")),
    };
    let mut restored = Explorer::new(original.language, Saved(saved));
    let startup_count = restored.language.open_count;
    let before = restored.session.clone();
    let session_path = Path::new("session.json");
    let error = restored
        .load_project_session(
            session_path,
            &std::env::temp_dir(),
            &ProjectOptions::default(),
        )
        .unwrap_err();
    assert!(error.contains("does not match selected project"));
    assert_eq!(restored.language.open_count, startup_count);
    assert_eq!(restored.session, before);

    let selected_database = root.join("selected/compile_commands.json");
    let overrides = ProjectOptions {
        language: ProjectLanguage::Auto,
        compilation_database: Some(selected_database.clone()),
    };
    restored
        .load_project_session(session_path, &root, &overrides)
        .unwrap();
    assert_eq!(restored.language.open_count, startup_count + 1);
    assert_eq!(restored.language.options.language, ProjectLanguage::Cpp);
    assert_eq!(
        restored.session.project_options.compilation_database,
        Some(selected_database)
    );

    let rust = ProjectOptions {
        language: ProjectLanguage::Rust,
        compilation_database: None,
    };
    restored
        .load_project_session(session_path, &root, &rust)
        .unwrap();
    assert_eq!(restored.language.options, rust);
    assert_eq!(restored.session.project_options, rust);

    restored.repository.0.project_options = rust;
    restored
        .load_project_session(session_path, &root, &overrides)
        .unwrap();
    assert_eq!(restored.language.options.language, ProjectLanguage::Cpp);
    assert_eq!(restored.session.project_options, restored.language.options);
    assert_eq!(
        restored.session.project_options.compilation_database,
        overrides.compilation_database
    );

    let incompatible = ProjectOptions {
        language: ProjectLanguage::Rust,
        compilation_database: Some(root.join("compile_commands.json")),
    };
    let startup_count = restored.language.open_count;
    let before = restored.session.clone();
    assert!(
        restored
            .load_project_session(session_path, &root, &incompatible)
            .is_err()
    );
    assert_eq!(restored.language.open_count, startup_count);
    assert_eq!(restored.session, before);
}

#[test]
fn typescript_override_clears_saved_cpp_database_before_restoring() {
    struct Saved(Session);
    impl SessionRepository for Saved {
        fn save(&self, _: &Path, _: &Session) -> Result<()> {
            Ok(())
        }
        fn load(&self, _: &Path) -> Result<Session> {
            Ok(self.0.clone())
        }
    }
    let original = explorer();
    let root = original.session.project_root.clone();
    let mut session = original.session;
    session.project_options = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(root.join("compile_commands.json")),
    };
    let mut restored = Explorer::new(original.language, Saved(session));
    let typescript = ProjectOptions {
        language: ProjectLanguage::TypeScript,
        compilation_database: None,
    };
    restored
        .load_project_session(Path::new("session.json"), &root, &typescript)
        .unwrap();
    assert_eq!(restored.language.options, typescript);
    assert_eq!(restored.session.project_options, typescript);
    let before = restored.session.clone();
    let startup_count = restored.language.open_count;
    let invalid = ProjectOptions {
        compilation_database: Some(root.join("build")),
        ..typescript
    };
    assert!(
        restored
            .load_project_session(Path::new("session.json"), &root, &invalid)
            .is_err()
    );
    assert_eq!(restored.session, before);
    assert_eq!(restored.language.open_count, startup_count);
}

#[test]
fn saved_project_metadata_requires_a_valid_session_and_never_starts_backend() {
    struct Saved(Session);
    impl SessionRepository for Saved {
        fn save(&self, _: &Path, _: &Session) -> Result<()> {
            Ok(())
        }
        fn load(&self, _: &Path) -> Result<Session> {
            Ok(self.0.clone())
        }
    }
    let original = explorer();
    let mut saved = original.session.clone();
    saved.project_root = PathBuf::from("missing/source/folder");
    let mut inspected = Explorer::new(original.language, Saved(saved));
    let startup_count = inspected.language.open_count;
    let before = inspected.session.clone();
    assert_eq!(
        inspected
            .session_project_root(Path::new("session.json"))
            .unwrap(),
        PathBuf::from("missing/source/folder")
    );
    inspected.repository.0.version += 1;
    assert!(
        inspected
            .session_project_root(Path::new("session.json"))
            .is_err()
    );
    assert_eq!(inspected.language.open_count, startup_count);
    assert_eq!(inspected.session, before);
}

#[test]
fn python_session_restores_and_explicit_override_clears_saved_cpp_database() {
    struct Saved(Session);
    impl SessionRepository for Saved {
        fn save(&self, _: &Path, _: &Session) -> Result<()> {
            Ok(())
        }
        fn load(&self, _: &Path) -> Result<Session> {
            Ok(self.0.clone())
        }
    }
    let original = explorer();
    let root = original.session.project_root.clone();
    let python = ProjectOptions {
        language: ProjectLanguage::Python,
        compilation_database: None,
    };
    let mut saved = original.session;
    saved.project_options = python.clone();
    let mut restored = Explorer::new(original.language, Saved(saved.clone()));
    restored.load_session(Path::new("session.json")).unwrap();
    assert_eq!(restored.language.options, python);
    assert_eq!(restored.session, saved);

    restored.repository.0.project_options = ProjectOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(root.join("compile_commands.json")),
    };
    restored
        .load_project_session(Path::new("session.json"), &root, &python)
        .unwrap();
    assert_eq!(restored.language.options, python);
    assert_eq!(restored.session.project_options, python);
    let before = restored.session.clone();
    let startup_count = restored.language.open_count;
    let invalid = ProjectOptions {
        compilation_database: Some(root.join("build")),
        ..python
    };
    assert!(
        restored
            .load_project_session(Path::new("session.json"), &root, &invalid)
            .is_err()
    );
    assert_eq!(restored.language.open_count, startup_count);
    assert_eq!(restored.session, before);
}
