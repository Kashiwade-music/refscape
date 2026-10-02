//! Fixtures traverse the same public commands, backend ports and worker as native UI.
use super::*;
use refscape_application::test_support::*;
use refscape_application::{
    HeadlessDriver, ImportedSession, PersistableSession, SessionRepository, WorkerExecutor,
};
use refscape_model::{FeatureResult, OperationContext, ProjectCrate, ResolvedProjectOptions};
use std::sync::{Arc, Mutex};
pub(super) type Requests = Arc<Mutex<Vec<Position>>>;
pub(super) fn resolved_options(options: &ProjectOpenOptions) -> ResolvedProjectOptions {
    let mut options = options.clone();
    if options.language == ProjectLanguage::Auto {
        options.language = if options.compilation_database.is_some() {
            ProjectLanguage::Cpp
        } else {
            ProjectLanguage::Rust
        };
    }
    ResolvedProjectOptions::try_from(options).unwrap()
}
#[derive(Clone)]
pub(super) struct Language {
    pub(super) source: SourceDocument,
    pub(super) requests: Requests,
    pub(super) targets: Vec<Symbol>,
    pub(super) type_targets: Vec<Symbol>,
    pub(super) type_requests: Requests,
    pub(super) highlights: Vec<SourceRange>,
}
impl AnalysisFactory for Language {
    fn prepare(
        &self,
        root: &Path,
        options: &ProjectOpenOptions,
        _: &OperationContext,
    ) -> AnalysisResult<PreparedProject> {
        Ok(PreparedProject {
            root: root.into(),
            options: resolved_options(options),
            catalog: CatalogOutcome::Ready(vec![]),
            crates: vec![],
            capabilities: AnalysisCapabilities::default(),
            session: Box::new(self.clone()),
        })
    }
}
impl AnalysisSession for Language {
    fn project_options(&self) -> ResolvedProjectOptions {
        resolved_options(&ProjectOpenOptions::default())
    }
    fn files(&mut self, _: &OperationContext) -> AnalysisResult<Vec<PathBuf>> {
        Ok(vec![])
    }
    fn symbols(&mut self, _: &Path, _: &OperationContext) -> AnalysisResult<Vec<Symbol>> {
        Ok(vec![])
    }
    fn source(&mut self, symbol: &Symbol, _: &OperationContext) -> AnalysisResult<SourceDocument> {
        let mut source = self.source.clone();
        source.symbol = symbol.clone();
        Ok(source)
    }
    fn definitions(
        &mut self,
        _: &Path,
        position: Position,
        _: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationTarget>> {
        self.requests.lock().unwrap().push(position);
        Ok(self
            .targets
            .iter()
            .cloned()
            .map(navigation_target)
            .collect())
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
        _: &Path,
        position: Position,
        _: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Vec<NavigationTarget>>> {
        self.type_requests.lock().unwrap().push(position);
        Ok(FeatureResult::Supported(
            self.type_targets
                .iter()
                .cloned()
                .map(navigation_target)
                .collect(),
        ))
    }
    fn document_highlights(
        &mut self,
        _: &Path,
        _: Position,
        _: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Vec<SourceRange>>> {
        Ok(FeatureResult::Supported(self.highlights.clone()))
    }
    fn hover(
        &mut self,
        _: &Path,
        position: Position,
        _: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Option<String>>> {
        self.requests.lock().unwrap().push(position);
        if self.source.tokens.iter().any(|token| {
            token.line == position.line
                && token.start <= position.character
                && token
                    .start
                    .checked_add(token.length)
                    .is_some_and(|end| position.character < end)
                && matches!(token.kind.as_str(), "variable" | "parameter" | "property")
        }) {
            return Ok(FeatureResult::Supported(Some(format!(
                "let call: {}",
                if self.type_targets.is_empty() {
                    "u32"
                } else {
                    "Config"
                }
            ))));
        }
        Ok(FeatureResult::Supported(
            (position == Position::new(12, 9))
                .then(|| "fn call() -> u32\n\nCalls the helper.".into()),
        ))
    }
    fn project_crates(&mut self, _: &OperationContext) -> AnalysisResult<Vec<ProjectCrate>> {
        Ok(vec![])
    }
    fn search(&mut self, _: &str, _: &OperationContext) -> AnalysisResult<Vec<Symbol>> {
        Err("simulated analyzer failure".into())
    }
}
fn navigation_target(symbol: Symbol) -> NavigationTarget {
    NavigationTarget {
        location: NavigationLocation {
            document: symbol.path.clone(),
            target_range: symbol.range,
            selection_range: symbol.selection_range,
            origin_range: None,
        },
        symbol,
    }
}
pub(super) struct Repository;
impl SessionRepository for Repository {
    fn save(&self, _: &Path, _: &PersistableSession) -> refscape_application::Result<()> {
        Err("simulated storage failure".into())
    }
    fn load(&self, _: &Path) -> refscape_application::Result<ImportedSession> {
        Err("invalid fixture session".into())
    }
}
pub(super) struct FixtureDriver {
    pub(super) driver: HeadlessDriver,
}
impl FixtureDriver {
    pub(super) fn new(
        factory: impl AnalysisFactory + 'static,
        repository: impl SessionRepository + 'static,
    ) -> Self {
        Self {
            driver: HeadlessDriver::new(Arc::new(WorkerExecutor::new(
                Arc::new(factory),
                Arc::new(repository),
            ))),
        }
    }
    pub(super) fn snapshot(&self) -> &ApplicationSnapshot {
        self.driver.controller.snapshot()
    }
    pub(super) fn dispatch(
        &mut self,
        command: Command,
    ) -> refscape_application::Result<Vec<ViewEvent>> {
        let events = self.driver.dispatch(command);
        if self.driver.controller.error() {
            Err(self.driver.controller.status().to_owned().into())
        } else {
            Ok(events)
        }
    }
    pub(super) fn open_project(
        &mut self,
        root: &Path,
        options: &ProjectOpenOptions,
    ) -> refscape_application::Result<()> {
        self.dispatch(Command::OpenProject {
            root: root.into(),
            options: options.clone(),
            destination: "session.json".into(),
        })
        .map(|_| ())
    }
    pub(super) fn add_symbol(
        &mut self,
        symbol: Symbol,
        position: Point,
    ) -> refscape_application::Result<String> {
        let events = self.dispatch(Command::AddSymbol {
            symbol,
            position,
            toggle: false,
        })?;
        Ok(events
            .into_iter()
            .find_map(|event| match event {
                ViewEvent::Canvas(outcome) => outcome.targets.first().cloned(),
                _ => None,
            })
            .unwrap())
    }
    pub(super) fn move_card(
        &mut self,
        id: &str,
        position: Point,
    ) -> refscape_application::Result<()> {
        self.dispatch(Command::MoveCard {
            id: id.into(),
            position,
        })
        .map(|_| ())
    }
    pub(super) fn set_theme(&mut self, theme: Theme) -> refscape_application::Result<()> {
        self.dispatch(Command::SetTheme(theme)).map(|_| ())
    }
    pub(super) fn zoom(&mut self, factor: f32, anchor: Point) -> refscape_application::Result<()> {
        self.dispatch(Command::Zoom { factor, anchor }).map(|_| ())
    }
    pub(super) fn pan(&mut self, delta: Point) -> refscape_application::Result<()> {
        self.dispatch(Command::Pan(delta)).map(|_| ())
    }
    pub(super) fn expand_definition(
        &mut self,
        id: &str,
        position: Position,
    ) -> refscape_application::Result<Vec<String>> {
        let events = self.dispatch(Command::Navigate {
            card: id.into(),
            position,
            kind: ConnectionKind::Definition,
            anchor: Point::new(200.0, 60.0),
            toggle: false,
        })?;
        Ok(events
            .into_iter()
            .find_map(|event| match event {
                ViewEvent::Canvas(outcome) => Some(outcome.targets),
                _ => None,
            })
            .unwrap())
    }
}
impl ExplorerView {
    pub(super) fn from_fixture(
        fixture: FixtureDriver,
        session_path: PathBuf,
        themes: Vec<Theme>,
        project: Option<PathBuf>,
        options: ProjectOpenOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        {
            let initial = project.map(|root| Command::OpenProject {
                root,
                options: options.clone(),
                destination: session_path.clone(),
            });
            let (controller, executor) = fixture.driver.into_parts();
            Self::new(
                controller,
                executor,
                super::super::ViewLaunch {
                    session_path,
                    themes,
                    initial,
                    options,
                },
                window,
                cx,
            )
        }
    }
}
pub(super) fn fixture() -> (FixtureDriver, Requests) {
    fixture_with_targets(vec![])
}
pub(super) fn fixture_with_targets(targets: Vec<Symbol>) -> (FixtureDriver, Requests) {
    let range = SourceRange {
        start: Position::new(12, 5),
        end: Position::new(12, 13),
    };
    source_fixture(
        SourceDocument {
            expanded: vec![],
            folded: vec![],
            context: vec![],
            code_start: None,
            symbol: Symbol::file("sample.rs".into(), range),
            code: "日本😀call".into(),
            tokens: vec![],
        },
        targets,
    )
}
pub(super) fn source_fixture(
    source: SourceDocument,
    targets: Vec<Symbol>,
) -> (FixtureDriver, Requests) {
    let symbol = source.symbol.clone();
    let requests = Requests::default();
    let mut fixture = FixtureDriver::new(
        Language {
            source,
            requests: requests.clone(),
            targets,
            type_targets: vec![],
            type_requests: Requests::default(),
            highlights: vec![],
        },
        Repository,
    );
    fixture
        .open_project(Path::new("."), &ProjectOpenOptions::default())
        .unwrap();
    fixture.add_symbol(symbol, Point::new(100.0, 50.0)).unwrap();
    (fixture, requests)
}
pub(super) fn variable_fixture(has_type: bool) -> (FixtureDriver, Requests, Requests) {
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
        expanded: vec![],
        folded: vec![],
        context: vec![],
        code_start: None,
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
    let requests = Requests::default();
    let type_requests = Requests::default();
    let mut fixture = FixtureDriver::new(
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
    fixture
        .open_project(Path::new("."), &ProjectOpenOptions::default())
        .unwrap();
    fixture.add_symbol(symbol, Point::new(100.0, 50.0)).unwrap();
    (fixture, requests, type_requests)
}
