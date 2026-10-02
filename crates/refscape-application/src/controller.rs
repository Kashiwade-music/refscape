//! The sole mutable owner. Workers receive snapshots, never this controller.
use crate::{
    Command, Completion, Effect, NavigationMode, PersistableSession, Result, SaveDestination,
    editing::{CanvasEditOutcome, EditBasis, PreparedEdit, PresentationRevision},
    effect::{AnalysisQuery, AnalysisReply, ProjectRequest},
    jobs::{JobClass, JobRegistry},
    state::{ApplicationSnapshot, CanvasStore, ProjectState, VariableInspection},
};
use refscape_model::{
    CardId, ConnectionKind, ErrorKind, FoldToggle, JobId, MAX_ZOOM, MIN_ZOOM, OperationContext,
    Point, Position, ProjectCrate, ProjectEpoch, RefscapeError, Viewport,
};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};

#[derive(Default)]
pub struct Transition {
    pub effects: Vec<Effect>,
    pub events: Vec<ViewEvent>,
}
pub enum ViewEvent {
    Reset,
    Files(Vec<PathBuf>),
    Symbols(Vec<refscape_model::Symbol>),
    Inspection(VariableInspection),
    ClearInspection,
    Hover(Option<String>),
    Status { message: String, error: bool },
    Canvas(CanvasEditOutcome),
    CloseWindow,
}
struct LayoutUndo {
    basis: EditBasis,
    positions: Vec<(CardId, refscape_model::WorldPoint)>,
}
enum SaveAction {
    Manual,
    Switch(Box<ProjectRequest>),
    Close,
}
struct QueuedSave {
    context: OperationContext,
    path: PathBuf,
    snapshot: PersistableSession,
    action: SaveAction,
}

pub struct ApplicationController {
    project_state: ProjectState,
    metadata_revision: u64,
    snapshot: Arc<ApplicationSnapshot>,
    canvas: CanvasStore,
    basis: EditBasis,
    presentation: PresentationRevision,
    revision: u64,
    saved_revision: u64,
    next_epoch: u64,
    crates: Arc<Vec<ProjectCrate>>,
    jobs: JobRegistry,
    destination: SaveDestination,
    undo: Option<LayoutUndo>,
    selected: Option<CardId>,
    interaction: u64,
    dragging: bool,
    closing: bool,
    status: String,
    error: bool,
    pending_plan: Option<(PreparedEdit, JobClass)>,
    pending_project: Option<ProjectRequest>,
    pending_drop: Option<PreparedEdit>,
    saves: HashMap<PathBuf, VecDeque<QueuedSave>>,
    active_saves: HashMap<JobId, (PathBuf, SaveAction)>,
    latest_manual_save: Option<JobId>,
}
impl Default for ApplicationController {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for ApplicationController {
    fn drop(&mut self) {
        for record in self.jobs.records.values() {
            record.context.cancel.cancel();
        }
    }
}
impl ApplicationController {
    pub fn new() -> Self {
        let snapshot = Arc::new(ApplicationSnapshot::new(PathBuf::new()));
        Self {
            project_state: ProjectState::Empty,
            metadata_revision: 0,
            canvas: CanvasStore::index(&snapshot),
            snapshot,
            basis: EditBasis::default(),
            presentation: PresentationRevision::default(),
            revision: 0,
            saved_revision: 0,
            next_epoch: 0,
            crates: Arc::default(),
            jobs: JobRegistry::default(),
            destination: SaveDestination::Unset,
            undo: None,
            selected: None,
            interaction: 0,
            dragging: false,
            closing: false,
            status: "Ready".into(),
            error: false,
            pending_plan: None,
            pending_project: None,
            pending_drop: None,
            saves: HashMap::new(),
            active_saves: HashMap::new(),
            latest_manual_save: None,
        }
    }
    pub fn with_budget_policy(policy: crate::jobs::OperationBudgetPolicy) -> Self {
        let mut controller = Self::new();
        controller.jobs.policy = policy;
        controller
    }
    pub fn snapshot(&self) -> &ApplicationSnapshot {
        &self.snapshot
    }
    pub fn project_state(&self) -> &ProjectState {
        &self.project_state
    }
    pub fn is_open(&self) -> bool {
        matches!(self.project_state, ProjectState::Open { .. })
    }
    pub fn shared_snapshot(&self) -> Arc<ApplicationSnapshot> {
        self.snapshot.clone()
    }
    pub(crate) fn cancel_operations(&self) {
        for record in self.jobs.records.values() {
            record.context.cancel.cancel();
        }
    }
    pub fn basis(&self) -> EditBasis {
        self.basis
    }
    pub fn busy(&self) -> bool {
        self.jobs.busy()
            || !self.active_saves.is_empty()
            || self.pending_plan.is_some()
            || self.pending_drop.is_some()
    }
    pub fn closing(&self) -> bool {
        self.closing
    }
    pub fn dragging(&self) -> bool {
        self.dragging
    }
    pub fn destination(&self) -> &SaveDestination {
        &self.destination
    }
    pub fn pending_project(&self) -> Option<&ProjectRequest> {
        self.pending_project.as_ref()
    }
    pub fn pending_project_root(&self) -> Option<&std::path::Path> {
        match self.pending_project.as_ref()? {
            ProjectRequest::Fresh { root, .. } => Some(root),
            ProjectRequest::Loaded { loaded, .. } => Some(&loaded.snapshot.project_root),
            ProjectRequest::Saved { expected_root, .. } => expected_root.as_deref(),
        }
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn error(&self) -> bool {
        self.error
    }
    pub fn dirty(&self) -> bool {
        self.saved_revision != self.revision
    }
    pub fn can_undo_layout(&self) -> bool {
        self.undo
            .as_ref()
            .is_some_and(|undo| undo.basis == self.basis)
    }
    fn status_event(&mut self, message: impl Into<String>, error: bool, t: &mut Transition) {
        self.status = message.into();
        self.error = error;
        t.events.push(ViewEvent::Status {
            message: self.status.clone(),
            error,
        });
    }
    fn ready(&mut self, t: &mut Transition) {
        self.status_event(
            format!(
                "{} cards · {} connections · Ready",
                self.snapshot.cards.len(),
                self.snapshot.connections.len()
            ),
            false,
            t,
        );
    }
    fn fail(&mut self, error: impl ToString, t: &mut Transition) {
        self.status_event(error.to_string(), true, t);
    }
    fn cancel(&mut self, class: JobClass, t: &mut Transition) {
        for (id, _) in self.jobs.cancel_class(class) {
            t.effects.push(Effect::CancelJob { id });
        }
    }
    fn allows(&mut self, t: &mut Transition) -> bool {
        if self.closing {
            return false;
        }
        if self.busy() {
            self.status_event(
                "A request is running. Try again when it finishes.",
                false,
                t,
            );
            return false;
        }
        if self.dragging {
            self.status_event("Finish dragging before starting another request.", false, t);
            return false;
        }
        true
    }
    fn query(&mut self, class: JobClass, query: AnalysisQuery, t: &mut Transition) {
        let context = self.jobs.begin(class, self.basis.project);
        t.effects.push(Effect::QueryAnalysis {
            context,
            basis: self.basis,
            query,
        });
    }
    fn plan(&mut self, edit: PreparedEdit, class: JobClass, t: &mut Transition) {
        let edit = if class == JobClass::Arrange {
            PreparedEdit::Arrange {
                selected: self.selected.clone(),
            }
        } else {
            edit
        };
        let context = self.jobs.begin(class, self.basis.project);
        t.effects.push(Effect::PlanCanvas {
            context,
            basis: self.basis,
            snapshot: self.snapshot.clone(),
            crates: self.crates.clone(),
            edit,
            interaction: self.interaction,
        });
    }
    fn source(&self, id: &str, position: Position) -> Result<refscape_model::CardSource> {
        let card = self.canvas.card(&self.snapshot, id).ok_or_else(|| {
            RefscapeError::new(ErrorKind::InvalidData, format!("Unknown card {id}"))
        })?;
        if !card.source.contains_display_position(position) {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                "Requested source position is outside the card",
            ));
        }
        Ok(card.source.clone())
    }
    fn presentation_changed(&mut self) {
        self.presentation.0 += 1;
        self.revision += 1;
        self.interaction += 1;
    }
    pub fn dispatch(&mut self, command: Command) -> Transition {
        let mut t = Transition::default();
        if let Err(error) = self.dispatch_inner(command, &mut t) {
            self.fail(error, &mut t);
        }
        t
    }
    fn dispatch_inner(&mut self, command: Command, t: &mut Transition) -> Result<()> {
        match command {
            Command::Pan(delta) => {
                let viewport = Viewport {
                    offset: Point::new(
                        self.snapshot.viewport.offset.x + delta.x,
                        self.snapshot.viewport.offset.y + delta.y,
                    )
                    .try_into()?,
                    ..self.snapshot.viewport
                };
                viewport.validate()?;
                Arc::make_mut(&mut self.snapshot).viewport = viewport;
                self.presentation_changed();
            }
            Command::Zoom { factor, anchor } => {
                if !factor.is_finite() || factor <= 0.0 || !anchor.is_finite() {
                    return Err("Zoom requires a finite positive scale and anchor".into());
                }
                let old = self.snapshot.viewport;
                let zoom = (self.snapshot.viewport.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
                let viewport = Viewport {
                    offset: Point::new(
                        anchor.x - (anchor.x - old.offset.x) * zoom / old.zoom,
                        anchor.y - (anchor.y - old.offset.y) * zoom / old.zoom,
                    )
                    .try_into()?,
                    zoom,
                };
                viewport.validate()?;
                Arc::make_mut(&mut self.snapshot).viewport = viewport;
                self.presentation_changed();
            }
            Command::SetViewport(viewport) => {
                viewport.validate()?;
                Arc::make_mut(&mut self.snapshot).viewport = viewport;
                self.presentation_changed();
            }
            Command::Fit { width, height } => {
                if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
                    return Err("Fit dimensions must be finite and positive".into());
                }
                if !self.snapshot.cards.is_empty() {
                    let left = self
                        .snapshot
                        .cards
                        .iter()
                        .map(|c| c.position.x)
                        .fold(f32::INFINITY, f32::min);
                    let top = self
                        .snapshot
                        .cards
                        .iter()
                        .map(|c| c.position.y)
                        .fold(f32::INFINITY, f32::min);
                    let right = self
                        .snapshot
                        .cards
                        .iter()
                        .map(|c| c.position.x + c.width)
                        .fold(f32::NEG_INFINITY, f32::max);
                    let bottom = self
                        .snapshot
                        .cards
                        .iter()
                        .map(|c| c.position.y + c.display_height())
                        .fold(f32::NEG_INFINITY, f32::max);
                    let zoom = ((width.max(600.0) - 80.0) / (right - left).max(1.0))
                        .min((height.max(400.0) - 80.0) / (bottom - top).max(1.0))
                        .clamp(MIN_ZOOM, 2.0);
                    let viewport = Viewport {
                        zoom,
                        offset: Point::new(40.0 - left * zoom, 40.0 - top * zoom).try_into()?,
                    };
                    viewport.validate()?;
                    Arc::make_mut(&mut self.snapshot).viewport = viewport;
                    self.presentation_changed();
                } else {
                    Arc::make_mut(&mut self.snapshot).viewport = Viewport::default();
                    self.presentation_changed();
                }
            }
            Command::SetTheme(theme) => {
                theme.validate()?;
                Arc::make_mut(&mut self.snapshot).theme = theme;
                self.presentation_changed();
            }
            Command::Interaction { selected, dragging } => {
                let selected = selected.map(CardId::new).transpose()?;
                if self.selected != selected || self.dragging != dragging {
                    self.selected = selected;
                    self.dragging = dragging;
                    self.interaction += 1;
                }
                if !dragging
                    && !self.jobs.busy()
                    && self.pending_drop.is_none()
                    && let Some((edit, class)) = self.pending_plan.take()
                {
                    self.plan(edit, class, t);
                }
            }
            Command::CancelHover => {
                self.cancel(JobClass::Hover, t);
                t.events.push(ViewEvent::Hover(None));
            }
            Command::CancelInspection => {
                self.cancel(JobClass::Inspection, t);
                t.events.push(ViewEvent::ClearInspection);
            }
            Command::Hover { card, position } => {
                let card = CardId::new(card)?;
                self.cancel(JobClass::Hover, t);
                if !self.busy() && !self.closing && !self.dragging {
                    let source = self.source(&card, position)?;
                    self.query(
                        JobClass::Hover,
                        AnalysisQuery::Hover {
                            card,
                            source,
                            position,
                        },
                        t,
                    );
                }
            }
            Command::RequestClose => {
                self.request_close(t);
            }
            Command::Save => {
                if self.allows(t) {
                    self.save(None, SaveAction::Manual, t)?;
                }
            }
            Command::SaveAs(path) => {
                if self.allows(t) {
                    self.save(Some(path), SaveAction::Manual, t)?;
                }
            }
            Command::OpenProject {
                root,
                options,
                destination,
            } => {
                if self.allows(t) {
                    self.switch(
                        ProjectRequest::Fresh {
                            root,
                            options,
                            destination,
                        },
                        t,
                    )?;
                }
            }
            Command::OpenSession {
                path,
                expected_root,
                overrides,
            } => {
                if self.allows(t) {
                    self.switch(
                        ProjectRequest::Saved {
                            path,
                            expected_root,
                            overrides,
                        },
                        t,
                    )?;
                }
            }
            Command::OpenLoaded {
                loaded,
                destination,
                expected_root,
                overrides,
            } => {
                if self.allows(t) {
                    self.switch(
                        ProjectRequest::Loaded {
                            loaded: Box::new(loaded),
                            destination,
                            expected_root,
                            overrides,
                        },
                        t,
                    )?;
                }
            }
            Command::SetCompilationDatabase(database) => {
                if self.allows(t) {
                    let options = refscape_model::ProjectOpenOptions {
                        language: refscape_model::ProjectLanguage::Cpp,
                        compilation_database: Some(database),
                    };
                    let request = match self.pending_project.clone() {
                        Some(ProjectRequest::Fresh {
                            root, destination, ..
                        }) => ProjectRequest::Fresh {
                            root,
                            destination,
                            options,
                        },
                        Some(ProjectRequest::Saved {
                            path,
                            expected_root,
                            ..
                        }) => ProjectRequest::Saved {
                            path,
                            expected_root,
                            overrides: options,
                        },
                        Some(ProjectRequest::Loaded {
                            loaded,
                            destination,
                            expected_root,
                            ..
                        }) => ProjectRequest::Loaded {
                            loaded,
                            destination,
                            expected_root,
                            overrides: options,
                        },
                        None => ProjectRequest::Fresh {
                            root: self.snapshot.project_root.clone(),
                            destination: self
                                .destination
                                .path()
                                .ok_or("No project is open")?
                                .into(),
                            options,
                        },
                    };
                    self.switch(request, t)?;
                }
            }
            Command::Files => {
                if self.allows(t) {
                    self.query(JobClass::Query, AnalysisQuery::Files, t);
                }
            }
            Command::Symbols(path) => {
                if self.allows(t) {
                    self.query(JobClass::Query, AnalysisQuery::Symbols(path), t);
                }
            }
            Command::Search(query) => {
                self.cancel(JobClass::Search, t);
                if self.allows(t) {
                    self.query(JobClass::Search, AnalysisQuery::Search(query), t);
                }
            }
            Command::AddFile { path, position } => {
                if self.allows(t) {
                    if !position.is_finite() {
                        return Err("Card position must be finite".into());
                    }
                    let symbol =
                        refscape_model::Symbol::file(path, refscape_model::SourceRange::default());
                    if let Some(id) = self
                        .canvas
                        .symbol(&self.snapshot, &symbol)
                        .map(|card| card.id.to_string())
                    {
                        self.query(JobClass::Query, AnalysisQuery::Symbols(symbol.path), t);
                        t.events.push(ViewEvent::Canvas(CanvasEditOutcome {
                            targets: vec![id],
                            ..Default::default()
                        }));
                    } else {
                        self.query(
                            JobClass::Edit,
                            AnalysisQuery::Source {
                                symbol,
                                position,
                                list_symbols: true,
                            },
                            t,
                        );
                    }
                }
            }
            Command::AddSymbol {
                symbol,
                position,
                toggle,
            } => {
                if self
                    .jobs
                    .records
                    .values()
                    .any(|record| record.class == JobClass::Arrange)
                    && let Some(card) = self.canvas.symbol(&self.snapshot, &symbol)
                {
                    let id = card.id.clone();
                    self.selected = Some(id.clone());
                    self.interaction += 1;
                    t.events.push(ViewEvent::Canvas(CanvasEditOutcome {
                        targets: vec![id.to_string()],
                        ..Default::default()
                    }));
                    return Ok(());
                }
                if self.allows(t) {
                    symbol.validate()?;
                    if !position.is_finite() {
                        return Err("Card position must be finite".into());
                    }
                    if let Some(card) = self.canvas.symbol(&self.snapshot, &symbol) {
                        if toggle {
                            self.plan(
                                PreparedEdit::Hide {
                                    origin: None,
                                    targets: vec![card.id.clone()],
                                    connection: None,
                                },
                                JobClass::Edit,
                                t,
                            );
                        } else {
                            t.events.push(ViewEvent::Canvas(CanvasEditOutcome {
                                targets: vec![card.id.to_string()],
                                ..Default::default()
                            }));
                        }
                    } else {
                        self.query(
                            JobClass::Edit,
                            AnalysisQuery::Source {
                                symbol,
                                position,
                                list_symbols: false,
                            },
                            t,
                        );
                    }
                }
            }
            Command::Click {
                card,
                position,
                anchor,
                mode,
            } => {
                let card = CardId::new(card)?;
                if self.allows(t) {
                    let source = self.source(&card, position)?;
                    let position = if matches!(mode, NavigationMode::Normal) {
                        source
                            .variable_token(position)
                            .map_or(position, |token| Position::new(token.line, token.start))
                    } else {
                        position
                    };
                    let kind = match mode {
                        NavigationMode::Definition => ConnectionKind::Definition,
                        NavigationMode::References => ConnectionKind::Reference,
                        NavigationMode::Normal if source.variable_token(position).is_some() => {
                            self.cancel(JobClass::Inspection, t);
                            self.query(
                                JobClass::Inspection,
                                AnalysisQuery::Inspect {
                                    card: card.clone(),
                                    source: source.clone(),
                                    position,
                                },
                                t,
                            );
                            ConnectionKind::TypeDefinition
                        }
                        NavigationMode::Normal => ConnectionKind::Definition,
                    };
                    let position = crate::navigation::source_word(&source, position)
                        .and_then(|word| {
                            self.canvas
                                .outgoing
                                .get(card.as_str())
                                .into_iter()
                                .flatten()
                                .map(|index| &self.snapshot.connections[*index])
                                .find(|edge| {
                                    edge.kind == kind
                                        && crate::navigation::source_word(&source, edge.source)
                                            .as_ref()
                                            == Some(&word)
                                })
                                .map(|edge| edge.source)
                        })
                        .unwrap_or(position);
                    self.navigate(card, position, kind, anchor, true, t)?;
                }
            }
            Command::Navigate {
                card,
                position,
                kind,
                anchor,
                toggle,
            } => {
                let card = CardId::new(card)?;
                if self.allows(t) {
                    self.navigate(card, position, kind, anchor, toggle, t)?;
                }
            }
            Command::Inspect { card, position } => {
                let card = CardId::new(card)?;
                if self.allows(t) {
                    self.cancel(JobClass::Inspection, t);
                    let source = self.source(&card, position)?;
                    self.query(
                        JobClass::Inspection,
                        AnalysisQuery::Inspect {
                            card,
                            source,
                            position,
                        },
                        t,
                    );
                }
            }
            Command::ToggleFold {
                card,
                index,
                expand,
            } => {
                let card = CardId::new(card)?;
                if self.allows(t) {
                    let mut source = self
                        .canvas
                        .card(&self.snapshot, &card)
                        .ok_or("Unknown card")?
                        .source
                        .clone();
                    if expand == source.expanded_context(index).is_some() {
                        return Ok(());
                    }
                    match source.toggle_fold(index)? {
                        FoldToggle::MissingLegacy { .. } => self.query(
                            JobClass::Edit,
                            AnalysisQuery::Fold {
                                card,
                                index,
                                source,
                            },
                            t,
                        ),
                        _ => {
                            self.plan(PreparedEdit::Resize { id: card, source }, JobClass::Edit, t)
                        }
                    }
                }
            }
            Command::MoveCard { id, position } => {
                let id = CardId::new(id)?;
                if !position.is_finite() {
                    return Err("Card position must be finite".into());
                }
                if self.closing {
                    return Ok(());
                }
                self.dragging = false;
                if self.jobs.busy() {
                    self.pending_drop = Some(PreparedEdit::Move { id, position });
                } else {
                    self.plan(PreparedEdit::Move { id, position }, JobClass::Edit, t);
                }
            }
            Command::CloseCard { id } => {
                let id = CardId::new(id)?;
                if self.allows(t) {
                    if self.canvas.card(&self.snapshot, &id).is_none() {
                        return Err(format!("Unknown card {id}").into());
                    }
                    self.plan(
                        PreparedEdit::Hide {
                            origin: None,
                            targets: vec![id],
                            connection: None,
                        },
                        JobClass::Edit,
                        t,
                    );
                }
            }
            Command::Arrange { selected } => {
                let selected = selected.map(CardId::new).transpose()?;
                if self.allows(t) {
                    self.cancel(JobClass::Arrange, t);
                    self.selected = selected.clone();
                    self.interaction += 1;
                    self.plan(PreparedEdit::Arrange { selected }, JobClass::Arrange, t);
                }
            }
            Command::UndoLayout => {
                if self.allows(t) && self.can_undo_layout() {
                    let positions = self.undo.as_ref().unwrap().positions.clone();
                    self.plan(PreparedEdit::Undo { positions }, JobClass::Edit, t);
                }
            }
        }
        Ok(())
    }
    fn navigate(
        &mut self,
        card: CardId,
        position: Position,
        kind: ConnectionKind,
        anchor: Point,
        toggle: bool,
        t: &mut Transition,
    ) -> Result<()> {
        if !anchor.is_finite() {
            return Err("Symbol anchor must be finite".into());
        }
        let source = self.source(&card, position)?;
        let targets: Vec<_> = self
            .canvas
            .outgoing
            .get(card.as_str())
            .into_iter()
            .flatten()
            .filter_map(|index| {
                let edge = &self.snapshot.connections[*index];
                (edge.source == position && edge.kind == kind).then(|| edge.to.clone())
            })
            .collect();
        if toggle && !targets.is_empty() {
            self.plan(
                PreparedEdit::Hide {
                    origin: Some(card.clone()),
                    targets: targets.into_iter().filter(|id| id != &card).collect(),
                    connection: Some((position, kind)),
                },
                JobClass::Edit,
                t,
            );
        } else {
            let existing = self
                .snapshot
                .cards
                .iter()
                .map(|card| card.source.clone())
                .collect();
            self.query(
                JobClass::Edit,
                AnalysisQuery::Navigate {
                    card,
                    path: source.symbol.path.clone(),
                    position,
                    kind,
                    anchor,
                    existing,
                },
                t,
            );
        }
        Ok(())
    }
    fn switch(&mut self, request: ProjectRequest, t: &mut Transition) -> Result<()> {
        self.cancel(JobClass::Hover, t);
        self.cancel(JobClass::Inspection, t);
        self.cancel(JobClass::Arrange, t);
        let same_saved_path = match &request {
            ProjectRequest::Saved { path, .. } => self.destination.path() == Some(path.as_path()),
            ProjectRequest::Loaded { destination, .. } => {
                self.destination.path() == Some(destination.as_path())
            }
            ProjectRequest::Fresh { .. } => false,
        };
        if !same_saved_path && self.destination.writable().is_some() && self.is_open() {
            self.save(None, SaveAction::Switch(Box::new(request)), t)?;
        } else {
            self.prepare(request, t);
        }
        Ok(())
    }
    fn prepare(&mut self, request: ProjectRequest, t: &mut Transition) {
        self.pending_project = Some(request.clone());
        self.next_epoch += 1;
        let context = self
            .jobs
            .begin(JobClass::Switch, ProjectEpoch(self.next_epoch));
        t.effects.push(Effect::PrepareProject {
            context,
            request,
            theme: Box::new(self.snapshot.theme.clone()),
        });
        self.status_event("Starting language service and indexing project…", false, t);
    }
    fn save(
        &mut self,
        path: Option<PathBuf>,
        action: SaveAction,
        t: &mut Transition,
    ) -> Result<()> {
        if !self.is_open() {
            return Err("Session project root is empty".into());
        }
        let path = match path {
            Some(path) => path,
            None => self.destination.path().map(PathBuf::from).ok_or_else(|| {
                RefscapeError::new(
                    ErrorKind::Io,
                    "Session is protected. Use Save as to choose another location.",
                )
            })?,
        };
        if path.as_os_str().is_empty() {
            return Err("Save path is empty".into());
        }
        let context = self.jobs.begin(JobClass::Save, self.basis.project);
        if matches!(action, SaveAction::Manual) {
            self.latest_manual_save = Some(context.id);
        }
        let save = QueuedSave {
            context,
            path: path.clone(),
            snapshot: PersistableSession {
                snapshot: self.snapshot.clone(),
                epoch: self.basis.project.0,
                revision: self.revision,
            },
            action,
        };
        self.saves.entry(path.clone()).or_default().push_back(save);
        self.pump_save(&path, t);
        Ok(())
    }
    fn pump_save(&mut self, path: &PathBuf, t: &mut Transition) {
        if self.active_saves.values().any(|(active, _)| active == path) {
            return;
        }
        if let Some(save) = self.saves.get_mut(path).and_then(VecDeque::pop_front) {
            self.active_saves
                .insert(save.context.id, (save.path.clone(), save.action));
            t.effects.push(Effect::WriteSession {
                context: save.context,
                path: save.path,
                snapshot: save.snapshot,
            });
        }
    }
    fn request_close(&mut self, t: &mut Transition) {
        if self.closing {
            return;
        }
        if self.dragging || self.pending_plan.is_some() || self.pending_drop.is_some() {
            self.status_event("Finish moving the canvas before closing.", false, t);
            return;
        }
        if self.busy() {
            self.status_event("A request is running. Close again after it finishes so the complete session can be saved.",false,t);
            return;
        }
        self.cancel(JobClass::Hover, t);
        self.cancel(JobClass::Arrange, t);
        self.cancel(JobClass::Inspection, t);
        if !self.is_open() || self.destination.writable().is_none() {
            self.closing = true;
            t.effects.push(Effect::DisposeProject {
                epoch: self.basis.project,
            });
            t.effects.push(Effect::CloseWindow);
            t.events.push(ViewEvent::CloseWindow);
            return;
        }
        self.closing = true;
        self.status_event("Saving session before closing…", false, t);
        if let Err(error) = self.save(None, SaveAction::Close, t) {
            self.closing = false;
            self.fail(error, t);
        }
    }
}
#[path = "completion.rs"]
mod completion;
