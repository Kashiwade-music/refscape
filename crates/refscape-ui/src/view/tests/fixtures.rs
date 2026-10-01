use super::*;
pub(super) type Requests = Arc<Mutex<Vec<Position>>>;

pub(super) struct Language {
    pub(super) source: SourceDocument,
    pub(super) requests: Arc<Mutex<Vec<Position>>>,
    pub(super) targets: Vec<Symbol>,
    pub(super) type_targets: Vec<Symbol>,
    pub(super) type_requests: Arc<Mutex<Vec<Position>>>,
    pub(super) highlights: Vec<SourceRange>,
}
impl LanguageService for Language {
    fn open_project(&mut self, _: &Path, _: &ProjectOptions) -> Result<(), String> {
        Ok(())
    }
    fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        Ok(vec![])
    }
    fn symbols(&mut self, _: &Path) -> Result<Vec<Symbol>, String> {
        Ok(vec![])
    }
    fn source(&mut self, symbol: &Symbol) -> Result<SourceDocument, String> {
        let mut source = self.source.clone();
        source.symbol = symbol.clone();
        Ok(source)
    }
    fn definitions(&mut self, _: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.requests.lock().unwrap().push(position);
        Ok(self.targets.clone())
    }
    fn references(&mut self, _: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.requests.lock().unwrap().push(position);
        Ok(self.targets.clone())
    }
    fn type_definitions(&mut self, _: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.type_requests.lock().unwrap().push(position);
        Ok(self.type_targets.clone())
    }
    fn document_highlights(&mut self, _: &Path, _: Position) -> Result<Vec<SourceRange>, String> {
        Ok(self.highlights.clone())
    }
    fn hover(&mut self, _: &Path, position: Position) -> Result<Option<String>, String> {
        self.requests.lock().unwrap().push(position);
        if self.source.variable_token(position).is_some() {
            return Ok(Some(format!(
                "let call: {}",
                if self.type_targets.is_empty() {
                    "u32"
                } else {
                    "Config"
                }
            )));
        }
        Ok((position == Position::new(12, 9))
            .then(|| "fn call() -> u32\n\nCalls the helper.".into()))
    }
    fn search(&mut self, _: &str) -> Result<Vec<Symbol>, String> {
        Err("simulated analyzer failure".into())
    }
}
pub(super) struct Repository;
impl SessionRepository for Repository {
    fn save(&self, _: &Path, _: &Session) -> Result<(), String> {
        Err("simulated storage failure".into())
    }
    fn load(&self, _: &Path) -> Result<Session, String> {
        Err("invalid fixture session".into())
    }
}
pub(super) fn fixture() -> (Explorer<Language, Repository>, Arc<Mutex<Vec<Position>>>) {
    fixture_with_targets(vec![])
}
pub(super) fn fixture_with_targets(
    targets: Vec<Symbol>,
) -> (Explorer<Language, Repository>, Arc<Mutex<Vec<Position>>>) {
    let range = SourceRange {
        start: Position::new(12, 5),
        end: Position::new(12, 13),
    };
    let symbol = Symbol::file(PathBuf::from("sample.rs"), range);
    let source = SourceDocument {
        expanded: Vec::new(),
        folded: Vec::new(),
        context: Vec::new(),
        code_start: None,
        symbol: symbol.clone(),
        code: "日本😀call".into(),
        tokens: vec![],
    };
    source_fixture(source, targets)
}

pub(super) fn source_fixture(
    source: SourceDocument,
    targets: Vec<Symbol>,
) -> (Explorer<Language, Repository>, Requests) {
    let symbol = source.symbol.clone();
    let requests = Arc::new(Mutex::new(vec![]));
    let mut explorer = Explorer::new(
        Language {
            source,
            requests: requests.clone(),
            targets,
            type_targets: vec![],
            type_requests: Arc::new(Mutex::new(vec![])),
            highlights: vec![],
        },
        Repository,
    );
    explorer
        .add_symbol(symbol, Point::new(100.0, 50.0))
        .unwrap();
    (explorer, requests)
}

pub(super) fn variable_fixture(
    has_type: bool,
) -> (Explorer<Language, Repository>, Requests, Requests) {
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
        expanded: Vec::new(),
        folded: Vec::new(),
        context: Vec::new(),
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
    let requests = Arc::new(Mutex::new(vec![]));
    let type_requests = Arc::new(Mutex::new(vec![]));
    let mut explorer = Explorer::new(
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
    explorer
        .add_symbol(symbol, Point::new(100.0, 50.0))
        .unwrap();
    (explorer, requests, type_requests)
}
