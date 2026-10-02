//! Worker-side effect execution. Only analysis sessions are serialized; canvas state
//! stays on the controller thread and is never held across a backend request.
use crate::{
    ApplicationController, Command, Completion, Effect, EffectExecutor, ImportedSession, Result,
    SessionRepository, Transition,
    editing::{AcquiredNavigationTarget, PreparedEdit, plan_edit, same_symbol},
    effect::{AnalysisQuery, AnalysisReply, PreparedApplicationProject, ProjectRequest},
    state::{ApplicationSnapshot, VariableInspection},
};
use refscape_analysis::{AnalysisFactory, AnalysisMetadata, AnalysisSession, CatalogOutcome};
use refscape_canvas::{
    layout::{LayoutCard, LayoutRules, plan_restore_repair_with_context},
    regions::build_regions,
};
use refscape_model::{
    CardSource, ConnectionKind, ErrorKind, FeatureResult, FoldToggle, OperationContext, Position,
    ProjectEpoch, ProjectOpenOptions, RefscapeError, SourceContext, SourceRange, Symbol, Theme,
};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, TryLockError},
    time::Duration,
};

type SessionHandle = Arc<Mutex<Option<Box<dyn AnalysisSession>>>>;
pub struct WorkerExecutor {
    factory: Arc<dyn AnalysisFactory>,
    repository: Arc<dyn SessionRepository>,
    sessions: Mutex<HashMap<ProjectEpoch, SessionHandle>>,
}
impl WorkerExecutor {
    pub fn new(factory: Arc<dyn AnalysisFactory>, repository: Arc<dyn SessionRepository>) -> Self {
        Self {
            factory,
            repository,
            sessions: Mutex::default(),
        }
    }
    fn session(&self, epoch: ProjectEpoch) -> Result<SessionHandle> {
        self.sessions
            .lock()
            .map_err(|_| "Analysis worker registry poisoned")?
            .get(&epoch)
            .cloned()
            .ok_or_else(|| RefscapeError::new(ErrorKind::BackendUnavailable, "No project is open"))
    }
    fn query(
        &self,
        context: &OperationContext,
        query: AnalysisQuery,
    ) -> Result<(AnalysisReply, Option<AnalysisMetadata>)> {
        context.check()?;
        let handle = self.session(context.project)?;
        let mut session = loop {
            context.check()?;
            match handle.try_lock() {
                Ok(session) => break session,
                Err(TryLockError::Poisoned(_)) => return Err("Analysis worker poisoned".into()),
                Err(TryLockError::WouldBlock) => {
                    std::thread::park_timeout(Duration::from_millis(2))
                }
            }
        };
        let session = session.as_mut().ok_or_else(|| {
            RefscapeError::new(ErrorKind::BackendUnavailable, "Project has been disposed")
        })?;
        let result = run_query(session.as_mut(), query, context);
        let metadata = result
            .as_ref()
            .ok()
            .and_then(|_| session.metadata_snapshot());
        session.finish_operation(context);
        context.check()?;
        result.map(|reply| (reply, metadata))
    }
    fn prepare(
        &self,
        request: &mut ProjectRequest,
        theme: Theme,
        context: &OperationContext,
    ) -> Result<PreparedApplicationProject> {
        context.check()?;
        let (mut snapshot, destination, protection) = match request.clone() {
            ProjectRequest::Fresh {
                root,
                options,
                destination,
            } => {
                let root = canonical_root(&root)?;
                if destination.is_file() {
                    match self
                        .repository
                        .load(&destination)
                        .and_then(|loaded| prepare_import(loaded, Some(&root), &options, context))
                    {
                        Ok(snapshot) => (snapshot, destination, None),
                        Err(error) => {
                            let mut snapshot = ApplicationSnapshot::new(root);
                            snapshot.project_options = options;
                            (
                                snapshot,
                                destination,
                                Some(format!("Project opened; session restore failed: {error}")),
                            )
                        }
                    }
                } else {
                    let mut snapshot = ApplicationSnapshot::new(root);
                    snapshot.project_options = options;
                    (snapshot, destination, None)
                }
            }
            ProjectRequest::Saved {
                path,
                expected_root,
                overrides,
            } => {
                let loaded = self.repository.load(&path)?;
                (
                    prepare_import(loaded, expected_root.as_deref(), &overrides, context)?,
                    path,
                    None,
                )
            }
            ProjectRequest::Loaded {
                loaded,
                destination,
                expected_root,
                overrides,
            } => (
                prepare_import(*loaded, expected_root.as_deref(), &overrides, context)?,
                destination,
                None,
            ),
        };
        context.check()?;
        // A fresh canvas inherits the chosen theme; custom themes also survive
        // project switches just as in the original project-open workflow.
        if snapshot.cards.is_empty() || (theme != Theme::dark() && theme != Theme::light()) {
            snapshot.theme = theme;
        }
        *request = ProjectRequest::Loaded {
            loaded: Box::new(ImportedSession {
                snapshot: snapshot.clone(),
            }),
            destination: destination.clone(),
            expected_root: None,
            overrides: ProjectOpenOptions::default(),
        };
        let mut prepared =
            self.factory
                .prepare(&snapshot.project_root, &snapshot.project_options, context)?;
        context.check()?;
        snapshot.project_root = prepared.root;
        snapshot.project_options = prepared.options.to_open_options();
        let refresh = if snapshot.cards.is_empty() {
            Vec::new()
        } else {
            let refresh =
                crate::reload::refresh_sources(prepared.session.as_mut(), &snapshot.cards, context);
            prepared.session.finish_operation(context);
            refresh?
        };
        let refreshed = !refresh.is_empty();
        if refreshed {
            let patch = plan_edit(
                &snapshot,
                &prepared.crates,
                &PreparedEdit::Reload { cards: refresh },
                context,
            )?;
            snapshot.cards = patch.cards;
            snapshot.connections = patch.connections;
        }
        snapshot.regions = Arc::new(build_regions(
            &snapshot
                .cards
                .iter()
                .map(LayoutCard::try_from)
                .collect::<Result<Vec<_>>>()?,
            &snapshot.project_root,
            &prepared.crates,
        ));
        snapshot.validate()?;
        let (files, protection, listing_failed) = match prepared.catalog {
            CatalogOutcome::Ready(files) => (files, protection, false),
            CatalogOutcome::Failed(error) => {
                let message = format!("Project opened; source file listing failed: {error}");
                (
                    Vec::new(),
                    Some(match protection {
                        Some(previous) => format!("{previous}\n{message}"),
                        None => message,
                    }),
                    true,
                )
            }
        };
        self.sessions
            .lock()
            .map_err(|_| "Analysis worker registry poisoned")?
            .insert(
                context.project,
                Arc::new(Mutex::new(Some(prepared.session))),
            );
        Ok(PreparedApplicationProject {
            options: prepared.options,
            snapshot,
            destination,
            crates: prepared.crates,
            files,
            protection,
            listing_failed,
            refreshed,
        })
    }
}
impl EffectExecutor for WorkerExecutor {
    fn execute(&self, effect: Effect) -> Completion {
        match effect {
            Effect::PrepareProject {
                context,
                mut request,
                theme,
            } => {
                let result = self.prepare(&mut request, *theme, &context);
                Completion::ProjectPrepared {
                    context,
                    request,
                    result: Box::new(result),
                }
            }
            Effect::QueryAnalysis {
                context,
                basis,
                query,
            } => {
                let (result, metadata) = match self.query(&context, query) {
                    Ok((reply, metadata)) => (Ok(reply), metadata.map(Box::new)),
                    Err(error) => (Err(error), None),
                };
                Completion::AnalysisQueried {
                    context,
                    basis,
                    result,
                    metadata,
                }
            }
            Effect::PlanCanvas {
                context,
                basis,
                snapshot,
                crates,
                edit,
                interaction,
            } => {
                let result = plan_edit(&snapshot, &crates, &edit, &context);
                Completion::CanvasPlanned {
                    context,
                    basis,
                    edit,
                    interaction,
                    result,
                }
            }
            Effect::WriteSession {
                context,
                path,
                snapshot,
            } => {
                let epoch = snapshot.epoch;
                let revision = snapshot.revision;
                let result = context
                    .check()
                    .and_then(|()| self.repository.save(&path, &snapshot));
                Completion::SessionWritten {
                    context,
                    path,
                    epoch,
                    revision,
                    result,
                }
            }
            Effect::CancelJob { id } => Completion::Cancelled { id },
            Effect::DisposeProject { epoch } => {
                let handle = self
                    .sessions
                    .lock()
                    .ok()
                    .and_then(|mut sessions| sessions.remove(&epoch));
                if let Some(handle) = handle {
                    let context = OperationContext::detached(Duration::from_secs(5));
                    loop {
                        match handle.try_lock() {
                            Ok(mut holder) => {
                                let session = holder.take();
                                drop(holder);
                                if let Some(session) = session {
                                    let _ = session.dispose(&context);
                                }
                                break;
                            }
                            Err(TryLockError::Poisoned(_)) => break,
                            Err(TryLockError::WouldBlock) => {
                                if context.check().is_err() {
                                    break;
                                }
                                std::thread::park_timeout(Duration::from_millis(2));
                            }
                        }
                    }
                }
                Completion::Disposed { epoch }
            }
            Effect::CloseWindow => Completion::WindowClosed,
        }
    }
}
fn canonical_root(root: &Path) -> Result<PathBuf> {
    let root = root.canonicalize().map_err(|error| {
        RefscapeError::new(
            ErrorKind::Io,
            format!("Cannot open project {}: {error}", root.display()),
        )
        .with_path(root)
    })?;
    if !root.is_dir() {
        return Err(RefscapeError::new(
            ErrorKind::InvalidData,
            format!("Project root is not a directory: {}", root.display()),
        ));
    }
    Ok(root)
}
fn prepare_import(
    loaded: ImportedSession,
    expected_root: Option<&Path>,
    overrides: &ProjectOpenOptions,
    context: &OperationContext,
) -> Result<ApplicationSnapshot> {
    let mut snapshot = loaded.snapshot;
    snapshot.validate()?;
    let root = canonical_root(&snapshot.project_root)?;
    if let Some(expected) = expected_root {
        let expected = canonical_root(expected)?;
        if root != expected {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                format!(
                    "Session project {} does not match selected project {}",
                    root.display(),
                    expected.display()
                ),
            ));
        }
    }
    snapshot.project_root = root;
    snapshot.project_options = snapshot.project_options.merge_overrides(overrides)?;
    let geometry = snapshot
        .cards
        .iter()
        .map(LayoutCard::try_from)
        .collect::<Result<Vec<_>>>()?;
    let delta = plan_restore_repair_with_context(&geometry, LayoutRules::default(), context)?;
    if !delta.changes.is_empty() {
        let cards = Arc::make_mut(&mut snapshot.cards);
        for change in delta.changes {
            let card = cards
                .iter_mut()
                .find(|card| card.id == change.id)
                .ok_or("Restore repair card disappeared")?;
            card.position = change.after;
        }
    }
    Ok(snapshot)
}
fn optional<T: Default>(feature: FeatureResult<T>) -> T {
    match feature {
        FeatureResult::Supported(value) => value,
        FeatureResult::Unsupported => T::default(),
    }
}
pub(crate) fn acquire(
    session: &mut dyn AnalysisSession,
    symbol: &Symbol,
    context: &OperationContext,
) -> Result<CardSource> {
    symbol.validate()?;
    context.check()?;
    let source = CardSource::try_from(session.source(symbol, context)?)?;
    Ok(
        match session.document_fingerprint(&source.symbol.path, context)? {
            Some(fingerprint) => source.with_document_fingerprint(fingerprint),
            None => source,
        },
    )
}
fn run_query(
    session: &mut dyn AnalysisSession,
    query: AnalysisQuery,
    context: &OperationContext,
) -> Result<AnalysisReply> {
    match query {
        AnalysisQuery::RefreshSources(cards) => {
            let cards = crate::reload::refresh_sources(session, &cards, context)?;
            Ok(if cards.is_empty() {
                AnalysisReply::Unchanged
            } else {
                AnalysisReply::Edit {
                    edit: PreparedEdit::Reload { cards },
                    symbols: None,
                }
            })
        }
        AnalysisQuery::Files => Ok(AnalysisReply::Files(session.files(context)?)),
        AnalysisQuery::Symbols(path) => {
            Ok(AnalysisReply::Symbols(session.symbols(&path, context)?))
        }
        AnalysisQuery::Search(query) => {
            let mut symbols = session.search(&query, context)?;
            symbols.sort_by(|a, b| {
                a.name
                    .cmp(&b.name)
                    .then(a.path.cmp(&b.path))
                    .then(a.range.start.cmp(&b.range.start))
            });
            Ok(AnalysisReply::Search(symbols))
        }
        AnalysisQuery::Source {
            symbol,
            position,
            list_symbols,
        } => {
            let symbols = if list_symbols {
                Some(session.symbols(&symbol.path, context)?)
            } else {
                None
            };
            Ok(AnalysisReply::Edit {
                edit: PreparedEdit::Add {
                    source: acquire(session, &symbol, context)?,
                    position,
                },
                symbols,
            })
        }
        AnalysisQuery::Navigate {
            card,
            path,
            position,
            kind,
            anchor,
            existing,
        } => {
            let mut symbols = match kind {
                ConnectionKind::Definition => session.definitions(&path, position, context)?,
                ConnectionKind::Reference => session.references(&path, position, context)?,
                ConnectionKind::TypeDefinition => {
                    optional(session.type_definitions(&path, position, context)?)
                }
            };
            symbols.sort_by(|a, b| {
                a.symbol
                    .path
                    .cmp(&b.symbol.path)
                    .then(a.symbol.range.start.cmp(&b.symbol.range.start))
                    .then(a.symbol.range.end.cmp(&b.symbol.range.end))
                    .then(a.symbol.id.cmp(&b.symbol.id))
            });
            let mut sources: Vec<AcquiredNavigationTarget> = Vec::new();
            for target in symbols {
                context.check()?;
                let symbol = target.symbol;
                symbol.validate()?;
                if sources
                    .iter()
                    .any(|source| same_symbol(&source.source.symbol, &symbol))
                {
                    continue;
                }
                let source = if let Some(source) = existing
                    .iter()
                    .find(|source| same_symbol(&source.symbol, &symbol))
                {
                    source.clone()
                } else {
                    acquire(session, &symbol, context)?
                };
                sources.push(AcquiredNavigationTarget {
                    source,
                    location: target.location,
                });
            }
            Ok(AnalysisReply::Edit {
                edit: PreparedEdit::Expand {
                    origin: card,
                    position,
                    kind,
                    anchor,
                    sources,
                },
                symbols: None,
            })
        }
        AnalysisQuery::Fold {
            card,
            index,
            source,
        } => {
            let range = source
                .folded_range(index)
                .ok_or("This section has no hidden source")?;
            let file = session.source(
                &Symbol::file(source.symbol.path.clone(), SourceRange::default()),
                context,
            )?;
            file.validate()?;
            let start = file.code_start.unwrap_or(file.symbol.range.start).line;
            let offset = range
                .start
                .checked_sub(start)
                .ok_or("Hidden source is outside the document")?;
            let lines: Vec<_> = file
                .code
                .lines()
                .skip(offset as usize)
                .take((range.end - range.start) as usize)
                .collect();
            if lines.len() != (range.end - range.start) as usize {
                return Err("Hidden source is outside the document".into());
            }
            let gap = SourceContext {
                start_line: range.start,
                code: format!("{}\n", lines.join("\n")),
            };
            let tokens = file
                .tokens
                .into_iter()
                .filter(|token| range.contains(&token.line))
                .collect();
            let mut source = source.with_gap(index, gap, tokens)?;
            if !matches!(source.toggle_fold(index)?, FoldToggle::Expanded) {
                return Err("Gap could not be expanded".into());
            }
            Ok(AnalysisReply::Edit {
                edit: PreparedEdit::Resize { id: card, source },
                symbols: None,
            })
        }
        AnalysisQuery::Hover {
            card,
            source,
            position,
        } => {
            let value = optional(session.hover(&source.symbol.path, position, context)?);
            Ok(AnalysisReply::Hover {
                card,
                source,
                position,
                value,
            })
        }
        AnalysisQuery::Inspect {
            card,
            source,
            position,
        } => {
            let Some(token) = source.variable_token(position) else {
                return Ok(AnalysisReply::Inspection {
                    card,
                    source,
                    value: None,
                });
            };
            let position = Position::new(token.line, token.start);
            let selected = SourceRange {
                start: position,
                end: Position::new(
                    token.line,
                    token
                        .start
                        .checked_add(token.length)
                        .ok_or("Semantic token overflow")?,
                ),
            };
            let path = source.symbol.path.clone();
            let mut highlights = optional(session.document_highlights(&path, position, context)?);
            for range in &highlights {
                range.validate()?;
            }
            if !highlights.contains(&selected) {
                highlights.push(selected);
            }
            let description = optional(session.hover(&path, position, context)?);
            Ok(AnalysisReply::Inspection {
                card,
                source,
                value: Some(VariableInspection {
                    path,
                    position,
                    highlights,
                    description,
                }),
            })
        }
    }
}
/// The CLI/check path drives exactly the same commands and completion contracts.
pub struct HeadlessDriver {
    pub controller: ApplicationController,
    pub executor: Arc<dyn EffectExecutor>,
    dispose_on_drop: bool,
}
impl Drop for HeadlessDriver {
    fn drop(&mut self) {
        if !self.dispose_on_drop {
            return;
        }
        self.controller.cancel_operations();
        self.executor.execute(Effect::DisposeProject {
            epoch: self.controller.basis().project,
        });
    }
}
impl HeadlessDriver {
    pub fn new(executor: Arc<dyn EffectExecutor>) -> Self {
        Self {
            controller: ApplicationController::new(),
            executor,
            dispose_on_drop: true,
        }
    }
    pub fn into_parts(mut self) -> (ApplicationController, Arc<dyn EffectExecutor>) {
        self.dispose_on_drop = false;
        (std::mem::take(&mut self.controller), self.executor.clone())
    }
    pub fn dispatch(&mut self, command: Command) -> Vec<crate::ViewEvent> {
        let transition = self.controller.dispatch(command);
        self.drain(transition)
    }
    fn drain(&mut self, transition: Transition) -> Vec<crate::ViewEvent> {
        let mut events = transition.events;
        let mut effects: VecDeque<_> = transition.effects.into();
        while let Some(effect) = effects.pop_front() {
            let completion = self.executor.execute(effect);
            let transition = self.controller.complete(completion);
            events.extend(transition.events);
            effects.extend(transition.effects);
        }
        events
    }
}
