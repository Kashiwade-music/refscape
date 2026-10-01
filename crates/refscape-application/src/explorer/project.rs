use super::*;

impl<L: LanguageService, R: SessionRepository> Explorer<L, R> {
    pub fn open_project(&mut self, root: &Path, options: &ProjectOptions) -> Result<()> {
        let root = root
            .canonicalize()
            .map_err(|error| format!("Cannot open project {}: {error}", root.display()))?;
        if !root.is_dir() {
            return Err(format!(
                "Project root is not a directory: {}",
                root.display()
            ));
        }
        self.language.open_project(&root, options)?;
        let project_crates = self.language.project_crates()?;
        let theme = self.session.theme.clone();
        self.session = Session::new(root);
        self.session.project_options = self.language.project_options();
        self.session.theme = theme;
        self.project_crates = project_crates;
        Ok(())
    }

    pub fn save_session(&self, path: &Path) -> Result<()> {
        self.session.validate()?;
        self.repository.save(path, &self.session)
    }

    pub fn load_session(&mut self, path: &Path) -> Result<()> {
        let session = self.repository.load(path)?;
        self.restore_session(session)
    }

    /// Inspect the saved project without starting or changing the language backend.
    pub fn session_project_root(&self, path: &Path) -> Result<PathBuf> {
        let session = self.repository.load(path)?;
        session.validate()?;
        Ok(session.project_root)
    }

    /// Restore a project's canvas with explicitly selected analysis settings.
    /// Root mismatches are rejected before any language server is started.
    pub fn load_project_session(
        &mut self,
        path: &Path,
        expected_root: &Path,
        overrides: &ProjectOptions,
    ) -> Result<()> {
        let mut session = self.repository.load(path)?;
        session.validate()?;
        let expected_root = expected_root
            .canonicalize()
            .map_err(|error| format!("Cannot open project {}: {error}", expected_root.display()))?;
        let saved_root = session.project_root.canonicalize().map_err(|error| {
            format!(
                "Cannot open session project {}: {error}",
                session.project_root.display()
            )
        })?;
        if saved_root != expected_root {
            return Err(format!(
                "Session project {} does not match selected project {}",
                saved_root.display(),
                expected_root.display()
            ));
        }
        session.project_root = saved_root;
        if overrides.language == ProjectLanguage::Rust && overrides.compilation_database.is_some() {
            return Err("A compilation database cannot be used with the Rust backend".into());
        }
        if overrides.language != ProjectLanguage::Auto {
            session.project_options.language = overrides.language;
        }
        if overrides.language == ProjectLanguage::Rust {
            session.project_options.compilation_database = None;
        } else if let Some(database) = &overrides.compilation_database {
            session.project_options.compilation_database = Some(database.clone());
            if overrides.language == ProjectLanguage::Auto {
                session.project_options.language = ProjectLanguage::Cpp;
            }
        }
        self.restore_session(session)
    }

    pub(super) fn restore_session(&mut self, mut session: Session) -> Result<()> {
        session.validate()?;
        arrange_cards(&mut session.cards)?;
        self.language
            .open_project(&session.project_root, &session.project_options)?;
        let effective_options = self.language.project_options();
        if effective_options != ProjectOptions::default() {
            session.project_options = effective_options;
        }
        let project_crates = self.language.project_crates()?;
        self.session = session;
        self.project_crates = project_crates;
        Ok(())
    }
}
