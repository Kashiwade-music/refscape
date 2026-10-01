use super::*;

/// A validated candidate; only the matching canvas version can receive it.
#[derive(Clone)]
pub struct PreparedLayoutCommit {
    generation: CanvasGeneration,
    pub changed: bool,
    session: Session,
    before: Vec<(String, Point)>,
}

impl PreparedLayoutCommit {
    pub fn generation(&self) -> CanvasGeneration {
        self.generation
    }
}

impl<L: LanguageService, R: SessionRepository> Explorer<L, R> {
    pub fn layout_snapshot(&self) -> CanvasLayoutSnapshot {
        CanvasLayoutSnapshot {
            session: self.session.clone(),
            generation: self.generation,
            project_crates: self.project_crates.clone(),
        }
    }

    pub fn plan_layout(&self, selected: Option<&str>) -> Result<PreparedLayoutCommit> {
        self.layout_snapshot().plan(selected)
    }
    pub fn apply_layout_commit(&mut self, mut commit: PreparedLayoutCommit) -> Result<bool> {
        if commit.generation != self.generation {
            return Err("Canvas changed while arranging layout".into());
        }
        if !commit.changed {
            return Ok(false);
        }
        commit.session.viewport = self.session.viewport;
        commit.session.theme = self.session.theme.clone();
        // A private candidate passed both validators on its worker. Matching all
        // versions proves those geometry/content inputs remain current at commit.
        self.session = commit.session;
        self.generation.geometry += 1;
        self.layout_undo = Some(LayoutUndo {
            generation: self.generation,
            positions: commit.before,
        });
        Ok(true)
    }

    pub fn arrange_layout(&mut self, selected: Option<&str>) -> Result<bool> {
        let plan = self.plan_layout(selected)?;
        self.apply_layout_commit(plan)
    }

    pub fn can_undo_layout(&self) -> bool {
        self.layout_undo
            .as_ref()
            .is_some_and(|undo| undo.generation == self.generation)
    }

    pub fn undo_layout(&mut self) -> Result<bool> {
        let Some(undo) = self
            .layout_undo
            .as_ref()
            .filter(|undo| undo.generation == self.generation)
        else {
            return Ok(false);
        };
        let mut session = self.session.clone();
        for (id, position) in &undo.positions {
            session
                .cards
                .iter_mut()
                .find(|card| &card.id == id)
                .ok_or("Layout undo no longer matches cards")?
                .position = *position;
        }
        session.validate()?;
        validate_layout(&session.cards, LayoutRules::default())?;
        self.session = session;
        self.rebuild_regions();
        self.geometry_changed();
        Ok(true)
    }
}

#[derive(Clone)]
pub struct CanvasLayoutSnapshot {
    session: Session,
    generation: CanvasGeneration,
    project_crates: Vec<ProjectCrate>,
}

impl CanvasLayoutSnapshot {
    pub fn generation(&self) -> CanvasGeneration {
        self.generation
    }
    /// Evaluate on a worker; no clocks or backend requests are involved.
    pub fn plan(&self, selected: Option<&str>) -> Result<PreparedLayoutCommit> {
        self.plan_cancellable(selected, &|| false)
    }

    pub fn plan_cancellable(
        &self,
        selected: Option<&str>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PreparedLayoutCommit> {
        if cancelled() {
            return Err("Layout calculation cancelled".into());
        }
        // The project picker has no cards to arrange before a project is opened.
        // Saving and every nonempty edit still require a valid project root.
        if self.session.project_root.as_os_str().is_empty()
            && self.session.cards.is_empty()
            && self.session.connections.is_empty()
        {
            let mut validation = self.session.clone();
            validation.project_root = PathBuf::from(".");
            validation.validate()?;
            return Ok(PreparedLayoutCommit {
                generation: self.generation,
                changed: false,
                session: self.session.clone(),
                before: Vec::new(),
            });
        }
        self.session.validate()?;
        let mut session = self.session.clone();
        let before = session
            .cards
            .iter()
            .map(|card| (card.id.clone(), card.position))
            .collect();
        let plan = plan_tree_arrangement_cancellable(
            &session.cards,
            &session.connections,
            selected,
            LayoutRules::default(),
            cancelled,
        )?;
        if cancelled() {
            return Err("Layout calculation cancelled".into());
        }
        plan.apply_positions(&mut session.cards, LayoutRules::default())?;
        session.regions =
            build_regions(&session.cards, &session.project_root, &self.project_crates);
        session.validate()?;
        let changed = session.cards != self.session.cards;
        Ok(PreparedLayoutCommit {
            generation: self.generation,
            changed,
            session,
            before,
        })
    }
}
