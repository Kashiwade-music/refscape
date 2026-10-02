use crate::*;

use refscape_analysis::*;
use refscape_model::*;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
#[derive(Default)]
struct FakeState {
    targets: Vec<Symbol>,
    navigation_targets: Option<Vec<NavigationTarget>>,
    sources: HashMap<String, SourceDocument>,
    code: String,
    fail_source: bool,
    fail_prepare: bool,
    fail_list: bool,
    fail_save: bool,
    source_calls: usize,
    prepare_calls: usize,
    load_calls: usize,
    writes: Vec<PersistableSession>,
    loaded: Option<ImportedSession>,
    crates: Vec<ProjectCrate>,
    metadata: Option<AnalysisMetadata>,
    finished_operations: usize,
    fingerprints: HashMap<PathBuf, DocumentFingerprint>,
    local_sources: bool,
}
#[derive(Clone)]
struct Factory(Arc<Mutex<FakeState>>);
impl AnalysisFactory for Factory {
    fn prepare(
        &self,
        root: &Path,
        options: &ProjectOpenOptions,
        context: &OperationContext,
    ) -> AnalysisResult<PreparedProject> {
        context.check()?;
        let mut state = self.0.lock().unwrap();
        state.prepare_calls += 1;
        if state.fail_prepare {
            return Err(RefscapeError::new(
                ErrorKind::BackendUnavailable,
                "backend unavailable",
            ));
        }
        let catalog = if state.fail_list {
            CatalogOutcome::Failed("listing failed".into())
        } else {
            CatalogOutcome::Ready(vec![root.join("origin.rs")])
        };
        let mut options = options.clone();
        if options.language == ProjectLanguage::Auto {
            options.language = ProjectLanguage::Rust;
        }
        let options = ResolvedProjectOptions::try_from(options)?;
        Ok(PreparedProject {
            root: root.into(),
            options: options.clone(),
            catalog,
            crates: state.crates.clone(),
            capabilities: AnalysisCapabilities::default(),
            session: Box::new(Backend {
                state: self.0.clone(),
                options: options.clone(),
            }),
        })
    }
}
struct Backend {
    state: Arc<Mutex<FakeState>>,
    options: ResolvedProjectOptions,
}
impl AnalysisSession for Backend {
    fn supports_source_reload(&self) -> bool {
        self.state.lock().unwrap().local_sources
    }
    fn document_fingerprint(
        &mut self,
        path: &Path,
        _: &OperationContext,
    ) -> AnalysisResult<Option<DocumentFingerprint>> {
        Ok(self.state.lock().unwrap().fingerprints.get(path).copied())
    }
    fn metadata_snapshot(&self) -> Option<AnalysisMetadata> {
        self.state.lock().unwrap().metadata.clone()
    }
    fn finish_operation(&mut self, _: &OperationContext) {
        self.state.lock().unwrap().finished_operations += 1;
    }
    fn project_options(&self) -> ResolvedProjectOptions {
        self.options.clone()
    }
    fn files(&mut self, _: &OperationContext) -> AnalysisResult<Vec<PathBuf>> {
        Ok(vec![symbol("origin").path])
    }
    fn symbols(&mut self, _: &Path, _: &OperationContext) -> AnalysisResult<Vec<Symbol>> {
        Ok(self.state.lock().unwrap().targets.clone())
    }
    fn source(
        &mut self,
        symbol: &Symbol,
        context: &OperationContext,
    ) -> AnalysisResult<SourceDocument> {
        context.check()?;
        let mut state = self.state.lock().unwrap();
        state.source_calls += 1;
        if state.fail_source {
            return Err("backend unavailable".into());
        }
        if let Some(source) = state.sources.get(&symbol.id) {
            return Ok(source.clone());
        }
        Ok(SourceDocument {
            symbol: symbol.clone(),
            code: if state.code.is_empty() {
                "fn target() {}".into()
            } else {
                state.code.clone()
            },
            tokens: Vec::new(),
            context: Vec::new(),
            code_start: None,
            folded: Vec::new(),
            expanded: Vec::new(),
        })
    }
    fn definitions(
        &mut self,
        _: &Path,
        _: Position,
        _: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationTarget>> {
        let state = self.state.lock().unwrap();
        if state.fail_source {
            return Err("backend unavailable".into());
        }
        Ok(state.navigation_targets.clone().unwrap_or_else(|| {
            state
                .targets
                .iter()
                .map(|symbol| NavigationTarget {
                    symbol: symbol.clone(),
                    location: NavigationLocation {
                        document: symbol.path.clone(),
                        target_range: symbol.range,
                        selection_range: symbol.selection_range,
                        origin_range: None,
                    },
                })
                .collect()
        }))
    }
    fn references(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationTarget>> {
        self.definitions(path, position, context)
    }
    fn type_definitions(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Vec<NavigationTarget>>> {
        Ok(FeatureResult::Supported(
            self.definitions(path, position, context)?,
        ))
    }
    fn hover(
        &mut self,
        _: &Path,
        _: Position,
        _: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Option<String>>> {
        Ok(FeatureResult::Supported(Some("hover".into())))
    }
    fn project_crates(&mut self, _: &OperationContext) -> AnalysisResult<Vec<ProjectCrate>> {
        Ok(self.state.lock().unwrap().crates.clone())
    }
    fn search(&mut self, _: &str, _: &OperationContext) -> AnalysisResult<Vec<Symbol>> {
        Ok(self.state.lock().unwrap().targets.clone())
    }
}
struct Repository(Arc<Mutex<FakeState>>);
impl SessionRepository for Repository {
    fn save(&self, _: &Path, session: &PersistableSession) -> Result<()> {
        let mut state = self.0.lock().unwrap();
        if state.fail_save {
            return Err("write failed".into());
        }
        state.writes.push(session.clone());
        Ok(())
    }
    fn load(&self, _: &Path) -> Result<ImportedSession> {
        let mut state = self.0.lock().unwrap();
        state.load_calls += 1;
        state.loaded.clone().ok_or_else(|| "no session".into())
    }
}
struct Harness {
    driver: HeadlessDriver,
    state: Arc<Mutex<FakeState>>,
    root: PathBuf,
}
impl Harness {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "refscape-controller-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let state = Arc::new(Mutex::new(FakeState {
            targets: vec![symbol("target")],
            ..Default::default()
        }));
        let executor = Arc::new(WorkerExecutor::new(
            Arc::new(Factory(state.clone())),
            Arc::new(Repository(state.clone())),
        ));
        let mut result = Self {
            driver: HeadlessDriver::new(executor),
            state,
            root,
        };
        result.run(Command::OpenProject {
            root: result.root.clone(),
            options: ProjectOpenOptions::default(),
            destination: result.root.join("session.json"),
        });
        result
    }
    fn snapshot(&self) -> &ApplicationSnapshot {
        self.driver.controller.snapshot()
    }
    fn run(&mut self, command: Command) -> Vec<ViewEvent> {
        self.driver.dispatch(command)
    }
    fn add(&mut self, name: &str, position: Point) -> String {
        let events = self.run(Command::AddSymbol {
            symbol: symbol(name),
            position,
            toggle: false,
        });
        events
            .into_iter()
            .find_map(|event| match event {
                ViewEvent::Canvas(outcome) => outcome.targets.into_iter().next(),
                _ => None,
            })
            .unwrap()
    }
    fn expand(&mut self, origin: &str, line: u32, target: &str) -> String {
        self.state.lock().unwrap().targets = vec![symbol(target)];
        let anchor = Point::new(
            self.snapshot()
                .cards
                .iter()
                .find(|card| card.id == origin)
                .unwrap()
                .width,
            60.0 + line as f32 * 20.0,
        );
        let events = self.run(Command::Navigate {
            card: origin.into(),
            position: Position::new(line, 4),
            kind: ConnectionKind::Definition,
            anchor,
            toggle: false,
        });
        events
            .into_iter()
            .find_map(|event| match event {
                ViewEvent::Canvas(outcome) => outcome.targets.into_iter().next(),
                _ => None,
            })
            .unwrap()
    }
    fn card(&self, name: &str) -> &CodeCard {
        self.snapshot()
            .cards
            .iter()
            .find(|card| card.source.symbol.name == name)
            .unwrap()
    }
    fn pending(&mut self, command: Command) -> Vec<Effect> {
        self.driver.controller.dispatch(command).effects
    }
    fn execute(&self, effect: Effect) -> Completion {
        self.driver.executor.execute(effect)
    }
    fn finish(&mut self, completion: Completion) -> Transition {
        self.driver.controller.complete(completion)
    }
    fn drain(&mut self, transition: Transition) -> Vec<ViewEvent> {
        let mut events = transition.events;
        let mut pending: std::collections::VecDeque<_> = transition.effects.into();
        while let Some(effect) = pending.pop_front() {
            let completion = self.execute(effect);
            let transition = self.finish(completion);
            events.extend(transition.events);
            pending.extend(transition.effects);
        }
        events
    }
    fn import(&mut self, snapshot: ApplicationSnapshot) {
        self.run(Command::OpenLoaded {
            loaded: ImportedSession { snapshot },
            destination: self.root.join("session.json"),
            expected_root: None,
            overrides: ProjectOpenOptions::default(),
        });
    }
}
fn symbol(name: &str) -> Symbol {
    Symbol {
        id: name.into(),
        name: name.into(),
        kind: "function".into(),
        path: PathBuf::from(format!("/project/{name}.rs")),
        range: SourceRange {
            start: Position::default(),
            end: Position::new(0, 14),
        },
        selection_range: SourceRange {
            start: Position::new(0, 3),
            end: Position::new(0, 9),
        },
        children: Vec::new(),
    }
}
fn document(symbol: Symbol, code: &str) -> SourceDocument {
    SourceDocument {
        symbol,
        code: code.into(),
        tokens: Vec::new(),
        context: Vec::new(),
        code_start: None,
        folded: Vec::new(),
        expanded: Vec::new(),
    }
}
fn unchanged_content(current: &ApplicationSnapshot, before: &ApplicationSnapshot) {
    assert_eq!(current.cards.len(), before.cards.len());
    for (card, original) in current.cards.iter().zip(before.cards.iter()) {
        assert_eq!(card.id, original.id);
        assert_eq!(card.source, original.source);
        assert_eq!((card.width, card.height), (original.width, original.height));
    }
    assert_eq!(current.connections, before.connections);
}
fn connected() -> Harness {
    let mut h = Harness::new();
    let mut root = symbol("root");
    root.range.end = Position::new(30, 0);
    h.state.lock().unwrap().sources.insert(
        "root".into(),
        document(
            root.clone(),
            &std::iter::repeat_n("    call();", 30)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    );
    h.run(Command::AddSymbol {
        symbol: root,
        position: Point::new(1000.0, 500.0),
        toggle: false,
    });
    let root = h.card("root").id.to_string();
    let later = h.expand(&root, 19, "later");
    let grand = h.expand(&later, 0, "grandchild");
    let earlier = h.expand(&root, 4, "earlier");
    h.add("unrelated", Point::new(-1000.0, -1000.0));
    for (id, position) in [
        (later, Point::new(5000.0, 3000.0)),
        (grand, Point::new(8000.0, 6000.0)),
        (earlier, Point::new(4000.0, 4500.0)),
    ] {
        h.run(Command::MoveCard { id, position });
    }
    h
}
mod concurrency;
mod layout;
mod lifecycle;
mod navigation;
mod reload;
