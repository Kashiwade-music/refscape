//! Backend preparation is separate from geometry planning and atomic commit.
use super::*;

#[derive(Debug, Clone, Default)]
pub struct CanvasEditOutcome {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub targets: Vec<String>,
    pub origin: Option<String>,
    pub expanded: Option<bool>,
}

#[derive(Clone)]
pub struct PreparedCanvasEdit {
    generation: CanvasGeneration,
    operation: Edit,
}

#[derive(Clone)]
enum Edit {
    Add {
        source: SourceDocument,
        position: Point,
    },
    Expand {
        origin: String,
        position: Position,
        kind: ConnectionKind,
        anchor: Point,
        sources: Vec<SourceDocument>,
    },
    Hide {
        origin: Option<String>,
        targets: Vec<String>,
        connection: Option<(Position, ConnectionKind)>,
    },
    Resize {
        id: String,
        source: SourceDocument,
    },
    Move {
        id: String,
        position: Point,
    },
}

#[derive(Clone)]
pub struct PreparedCanvasCommit {
    generation: CanvasGeneration,
    pub outcome: CanvasEditOutcome,
    session: Session,
    invalidates_undo: bool,
}

impl PreparedCanvasCommit {
    pub fn generation(&self) -> CanvasGeneration {
        self.generation
    }
}

impl<L: LanguageService, R: SessionRepository> Explorer<L, R> {
    pub fn prepare_add_file(&mut self, path: &Path, position: Point) -> Result<PreparedCanvasEdit> {
        self.prepare_add_symbol(
            Symbol::file(path.to_path_buf(), SourceRange::default()),
            position,
        )
    }
    pub fn prepare_add_symbol(
        &mut self,
        symbol: Symbol,
        position: Point,
    ) -> Result<PreparedCanvasEdit> {
        if !position.is_finite() {
            return Err("Card position must be finite".into());
        }
        let source = if let Some(card) = self.card_for_symbol(&symbol) {
            card.source.clone()
        } else {
            self.language.source(&symbol)?
        };
        source.validate()?;
        Ok(PreparedCanvasEdit {
            generation: self.generation,
            operation: Edit::Add { source, position },
        })
    }

    pub fn prepare_toggle_symbol(
        &mut self,
        symbol: Symbol,
        position: Point,
    ) -> Result<PreparedCanvasEdit> {
        if let Some(card) = self.card_for_symbol(&symbol) {
            return Ok(PreparedCanvasEdit {
                generation: self.generation,
                operation: Edit::Hide {
                    origin: None,
                    targets: vec![card.id.clone()],
                    connection: None,
                },
            });
        }
        self.prepare_add_symbol(symbol, position)
    }

    pub fn prepare_move_card(&self, id: &str, position: Point) -> Result<PreparedCanvasEdit> {
        if !position.is_finite() || !self.session.cards.iter().any(|card| card.id == id) {
            return Err("Move requires a known card and finite position".into());
        }
        Ok(PreparedCanvasEdit {
            generation: self.generation,
            operation: Edit::Move {
                id: id.into(),
                position,
            },
        })
    }

    pub fn prepare_toggle_expansion(
        &mut self,
        id: &str,
        position: Position,
        kind: ConnectionKind,
        anchor_offset: Point,
    ) -> Result<PreparedCanvasEdit> {
        let mut targets: Vec<_> = self
            .session
            .connections
            .iter()
            .filter(|edge| edge.from == id && edge.source == position && edge.kind == kind)
            .map(|edge| edge.to.clone())
            .collect();
        targets.sort();
        targets.dedup();
        if targets.is_empty() {
            return self.prepare_expansion(id, position, kind, anchor_offset);
        }
        targets.retain(|target| target != id);
        Ok(PreparedCanvasEdit {
            generation: self.generation,
            operation: Edit::Hide {
                origin: Some(id.into()),
                targets,
                connection: Some((position, kind)),
            },
        })
    }

    pub fn prepare_expansion(
        &mut self,
        id: &str,
        position: Position,
        kind: ConnectionKind,
        anchor_offset: Point,
    ) -> Result<PreparedCanvasEdit> {
        if !anchor_offset.is_finite() {
            return Err("Symbol anchor must be finite".into());
        }
        let origin = self
            .session
            .cards
            .iter()
            .find(|card| card.id == id)
            .ok_or_else(|| format!("Unknown card {id}"))?;
        if !origin.source.contains_display_position(position) {
            return Err("Requested source position is outside the card".into());
        }
        let path = origin.source.symbol.path.clone();
        let symbols = match kind {
            ConnectionKind::Definition => self.language.definitions(&path, position)?,
            ConnectionKind::TypeDefinition => self.language.type_definitions(&path, position)?,
            ConnectionKind::Reference => self.language.references(&path, position)?,
        };
        let mut sources: Vec<SourceDocument> = Vec::new();
        for symbol in symbols {
            symbol.validate()?;
            let source = if let Some(card) = self.card_for_symbol(&symbol) {
                card.source.clone()
            } else {
                self.language.source(&symbol)?
            };
            source.validate()?;
            sources.push(source);
        }
        sources.sort_by(|a, b| {
            a.symbol
                .path
                .cmp(&b.symbol.path)
                .then(a.symbol.range.start.cmp(&b.symbol.range.start))
                .then(a.symbol.range.end.cmp(&b.symbol.range.end))
                .then(a.symbol.id.cmp(&b.symbol.id))
        });
        let mut unique: Vec<SourceDocument> = Vec::new();
        for source in sources {
            if !unique
                .iter()
                .any(|other| same_symbol(&other.symbol, &source.symbol))
            {
                unique.push(source);
            }
        }
        Ok(PreparedCanvasEdit {
            generation: self.generation,
            operation: Edit::Expand {
                origin: id.into(),
                position,
                kind,
                anchor: anchor_offset,
                sources: unique,
            },
        })
    }

    pub fn prepare_context(
        &mut self,
        id: &str,
        index: usize,
        expand: bool,
    ) -> Result<PreparedCanvasEdit> {
        let source = self.context_source(id, index, expand)?;
        Ok(PreparedCanvasEdit {
            generation: self.generation,
            operation: Edit::Resize {
                id: id.into(),
                source,
            },
        })
    }

    /// Compute against the latest confirmed coordinates without touching the backend.
    pub fn plan_prepared(&mut self, edit: PreparedCanvasEdit) -> Result<PreparedCanvasCommit> {
        if edit.generation.session != self.generation.session
            || edit.generation.content != self.generation.content
        {
            return Err("Prepared edit belongs to an outdated canvas".into());
        }
        let original = self.session.clone();
        let generation = self.generation;
        let undo = self.layout_undo.clone();
        let invalidates_undo = matches!(edit.operation, Edit::Move { .. } | Edit::Resize { .. });
        let result = self.stage_edit(edit.operation).and_then(|outcome| {
            self.validate_canvas()?;
            Ok(PreparedCanvasCommit {
                generation,
                outcome,
                session: self.session.clone(),
                invalidates_undo,
            })
        });
        self.session = original;
        self.generation = generation;
        self.layout_undo = undo;
        result
    }

    pub fn apply_commit(&mut self, mut commit: PreparedCanvasCommit) -> Result<CanvasEditOutcome> {
        if commit.generation != self.generation {
            return Err("Canvas changed while preparing placement".into());
        }
        commit.session.viewport = self.session.viewport;
        commit.session.theme = self.session.theme.clone();
        // The private candidate was fully validated during planning. Exact versions
        // ensure its source and geometry inputs are still current; keep final commit short.
        let content_changed = commit.session.connections != self.session.connections
            || commit.session.cards.len() != self.session.cards.len()
            || commit
                .session
                .cards
                .iter()
                .zip(&self.session.cards)
                .any(|(a, b)| {
                    a.id != b.id
                        || a.source != b.source
                        || a.width != b.width
                        || a.height != b.height
                });
        let geometry_changed = commit.session.cards != self.session.cards;
        self.session = commit.session;
        if content_changed {
            self.content_changed();
        } else if geometry_changed {
            self.geometry_changed();
        }
        if commit.invalidates_undo {
            self.layout_undo = None;
        }
        Ok(commit.outcome)
    }

    pub fn commit_prepared(&mut self, edit: PreparedCanvasEdit) -> Result<CanvasEditOutcome> {
        let commit = self.plan_prepared(edit)?;
        self.apply_commit(commit)
    }

    fn stage_edit(&mut self, operation: Edit) -> Result<CanvasEditOutcome> {
        let before: BTreeSet<_> = self
            .session
            .cards
            .iter()
            .map(|card| card.id.clone())
            .collect();
        let mut outcome = CanvasEditOutcome::default();
        match operation {
            Edit::Add { source, position } => {
                let target = if let Some(card) = self.card_for_symbol(&source.symbol) {
                    card.id.clone()
                } else {
                    let (width, height) = source_dimensions(&source);
                    let occupied: Vec<_> = self.session.cards.iter().map(CardRect::from).collect();
                    let rect = nearest_vacant_position(
                        position,
                        width,
                        height,
                        None,
                        &occupied,
                        LayoutRules::default(),
                    )?;
                    self.insert_source(source, rect)
                };
                outcome.targets.push(target);
            }
            Edit::Expand {
                origin,
                position,
                kind,
                anchor,
                sources,
            } => {
                let parent = self
                    .session
                    .cards
                    .iter()
                    .find(|card| card.id == origin)
                    .ok_or("Expansion source disappeared")?;
                let desired =
                    Point::new(parent.position.x + anchor.x, parent.position.y + anchor.y);
                let min_x = f64::from(parent.position.x)
                    + f64::from(parent.width)
                    + f64::from(LayoutRules::default().right_gap);
                for source in sources {
                    let target = if let Some(card) = self.card_for_symbol(&source.symbol) {
                        card.id.clone()
                    } else {
                        let (width, height) = source_dimensions(&source);
                        let occupied: Vec<_> =
                            self.session.cards.iter().map(CardRect::from).collect();
                        let rect = nearest_vacant_position(
                            desired,
                            width,
                            height,
                            Some(min_x),
                            &occupied,
                            LayoutRules::default(),
                        )?;
                        self.insert_source(source, rect)
                    };
                    if !outcome.targets.contains(&target) {
                        outcome.targets.push(target.clone());
                    }
                    if !self.session.connections.iter().any(|edge| {
                        edge.from == origin
                            && edge.to == target
                            && edge.source == position
                            && edge.kind == kind
                    }) {
                        let mut number = self.session.connections.len();
                        let id = loop {
                            let id = format!("connection:{number}");
                            if !self.session.connections.iter().any(|edge| edge.id == id) {
                                break id;
                            }
                            number += 1;
                        };
                        self.session.connections.push(Connection {
                            id,
                            from: origin.clone(),
                            to: target,
                            source: position,
                            kind,
                        });
                    }
                }
                outcome.origin = Some(origin);
                outcome.expanded = Some(true);
            }
            Edit::Hide {
                origin,
                targets,
                connection,
            } => {
                let preserved: Vec<_> = origin.iter().cloned().collect();
                self.remove_cards(&targets, &preserved)?;
                if let (Some(id), Some((position, kind))) = (&origin, connection) {
                    self.session.connections.retain(|edge| {
                        !(edge.from == *id && edge.source == position && edge.kind == kind)
                    });
                }
                outcome.origin = origin;
                outcome.expanded = Some(false);
            }
            Edit::Resize { id, source } => {
                let index = self
                    .session
                    .cards
                    .iter()
                    .position(|card| card.id == id)
                    .ok_or("Resized card disappeared")?;
                self.replace_card_source(index, source)?;
                outcome.origin = Some(id);
            }
            Edit::Move { id, position } => {
                self.move_card(&id, position)?;
                outcome.origin = Some(id);
            }
        }
        self.rebuild_regions();
        let after: BTreeSet<_> = self
            .session
            .cards
            .iter()
            .map(|card| card.id.clone())
            .collect();
        outcome.added = after.difference(&before).cloned().collect();
        outcome.removed = before.difference(&after).cloned().collect();
        Ok(outcome)
    }
}
