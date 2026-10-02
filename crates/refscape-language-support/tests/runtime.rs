use refscape_analysis::{AnalysisResult, CatalogOutcome, ErrorKind, OperationContext};
use refscape_language_support::{
    catalog::{WalkPolicy, walk},
    runtime::{LspAnalysisSession, Metadata, MetadataProvider, SearchMergePolicy},
};
use refscape_lsp::{ServerConfiguration, transport::DefaultServerBehavior};
use refscape_model::{ProjectLanguage, ProjectOpenOptions};
use serde_json::Value;
use std::{
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "refscape-adapter-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("a.py"), "alpha\n").unwrap();
        std::fs::write(path.join("b.py"), "bravo\n").unwrap();
        Self(path.canonicalize().unwrap())
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("refscape-adapter-")
        );
        assert_eq!(
            self.0.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Provider {
    root: PathBuf,
    policy: SearchMergePolicy,
    unavailable: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
}
impl MetadataProvider for Provider {
    fn refresh(&mut self, context: &OperationContext) -> AnalysisResult<Metadata> {
        context.check()?;
        if self.unavailable.load(Ordering::Acquire) {
            return Ok(Metadata {
                options: ProjectOpenOptions {
                    language: ProjectLanguage::Python,
                    compilation_database: None,
                }
                .try_into()?,
                files: vec![],
                catalog_error: Some(refscape_analysis::RefscapeError::new(
                    ErrorKind::Io,
                    "catalog unavailable",
                )),
                crates: vec![],
                seed: None,
                search: self.policy,
                prewarm_references: true,
                revision: self.generation.load(Ordering::Acquire),
            });
        }
        let files = walk(
            &self.root,
            WalkPolicy {
                extensions: &["py"],
                excluded: &[],
                case_insensitive: false,
                symlink_files: false,
                exclude_virtual_environments: false,
                canonical_paths: true,
            },
            false,
        )?;
        Ok(Metadata {
            options: ProjectOpenOptions {
                language: ProjectLanguage::Python,
                compilation_database: None,
            }
            .try_into()?,
            seed: files.first().cloned(),
            files,
            catalog_error: None,
            crates: if self.generation.load(Ordering::Acquire) == 0 {
                vec![]
            } else {
                vec![refscape_model::ProjectCrate {
                    id: "stable-package-id".into(),
                    name: "renamed-package".into(),
                    root: self.root.clone(),
                }]
            },
            search: self.policy,
            prewarm_references: true,
            revision: self.generation.load(Ordering::Acquire),
        })
    }
}
fn context() -> OperationContext {
    OperationContext::detached(Duration::from_secs(5))
}
fn prepare(
    files: &Files,
    scenario: &str,
    policy: SearchMergePolicy,
) -> refscape_analysis::PreparedProject {
    prepare_provider(
        files,
        scenario,
        Box::new(Provider {
            root: files.0.clone(),
            policy,
            unavailable: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
        }),
    )
    .unwrap()
}
fn prepare_provider(
    files: &Files,
    scenario: &str,
    provider: Box<dyn MetadataProvider>,
) -> AnalysisResult<refscape_analysis::PreparedProject> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_refscape-language-fixture"));
    command
        .arg(scenario)
        .env("REFSCAPE_FIXTURE_LOG", files.0.join("rpc.jsonl"));
    LspAnalysisSession::prepare(
        files.0.clone(),
        command,
        ServerConfiguration {
            name: "fixture".into(),
            installation_hint: "fixture".into(),
            initialization_options: Value::Null,
            experimental_capabilities: Value::Null,
            language_id: |_| "python",
            behavior: Box::new(DefaultServerBehavior),
        },
        provider,
        &context(),
    )
}
fn count(files: &Files, method: &str) -> usize {
    std::fs::read_to_string(files.0.join("rpc.jsonl"))
        .unwrap()
        .lines()
        .filter(|line| serde_json::from_str::<Value>(line).unwrap()["method"] == method)
        .count()
}
#[test]
fn warm_search_indexes_only_new_dirty_documents_and_closes_removed_documents() {
    for policy in [
        SearchMergePolicy::WorkspaceFirst,
        SearchMergePolicy::DocumentsFirst,
    ] {
        let files = Files::new();
        let prepared = prepare(&files, "normal", policy);
        assert!(matches!(prepared.catalog,CatalogOutcome::Ready(ref files) if files.len()==2));
        let mut session = prepared.session;
        assert_eq!(session.search("sample", &context()).unwrap().len(), 2);
        assert_eq!(count(&files, "textDocument/documentSymbol"), 2);
        assert_eq!(session.search("", &context()).unwrap().len(), 2);
        assert_eq!(count(&files, "textDocument/documentSymbol"), 2);
        std::fs::write(files.0.join("b.py"), "delta\n").unwrap();
        assert_eq!(session.search("SAMPLE", &context()).unwrap().len(), 2);
        assert_eq!(count(&files, "textDocument/documentSymbol"), 3);
        assert_eq!(count(&files, "textDocument/didChange"), 1);
        std::fs::rename(files.0.join("b.py"), files.0.join("c.py")).unwrap();
        let symbols = session.search("sample", &context()).unwrap();
        assert_eq!(symbols.len(), 2);
        assert!(symbols.iter().all(|symbol| !symbol.path.ends_with("b.py")));
        assert_eq!(count(&files, "textDocument/didClose"), 1);
        assert_eq!(count(&files, "textDocument/documentSymbol"), 4);
        let original = session.source(&symbols[0], &context()).unwrap().code;
        std::fs::write(files.0.join("a.py"), "later\n").unwrap();
        session.files(&context()).unwrap();
        assert_eq!(original, "alpha");
    }
}
#[test]
fn malformed_document_symbols_fail_candidate_instead_of_publishing_partial_results() {
    let files = Files::new();
    let mut command = Command::new(env!("CARGO_BIN_EXE_refscape-language-fixture"));
    command.arg("malformed");
    let result = LspAnalysisSession::prepare(
        files.0.clone(),
        command,
        ServerConfiguration {
            name: "fixture".into(),
            installation_hint: "fixture".into(),
            initialization_options: Value::Null,
            experimental_capabilities: Value::Null,
            language_id: |_| "python",
            behavior: Box::new(DefaultServerBehavior),
        },
        Box::new(Provider {
            root: files.0.clone(),
            policy: SearchMergePolicy::DocumentsFirst,
            unavailable: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
        }),
        &context(),
    );
    assert_eq!(result.err().unwrap().kind, ErrorKind::Protocol);
}
#[test]
fn listing_failure_retains_opened_backend_and_recovers_without_replacing_saved_source() {
    let files = Files::new();
    let unavailable = Arc::new(AtomicBool::new(true));
    let command = Command::new(env!("CARGO_BIN_EXE_refscape-language-fixture"));
    let prepared = LspAnalysisSession::prepare(
        files.0.clone(),
        command,
        ServerConfiguration {
            name: "fixture".into(),
            installation_hint: "fixture".into(),
            initialization_options: Value::Null,
            experimental_capabilities: Value::Null,
            language_id: |_| "python",
            behavior: Box::new(DefaultServerBehavior),
        },
        Box::new(Provider {
            root: files.0.clone(),
            policy: SearchMergePolicy::DocumentsFirst,
            unavailable: unavailable.clone(),
            generation: Arc::new(AtomicU64::new(0)),
        }),
        &context(),
    )
    .unwrap();
    assert!(
        matches!(prepared.catalog, CatalogOutcome::Failed(ref error) if error.kind == ErrorKind::Io)
    );
    let mut session = prepared.session;
    let symbols = session.symbols(&files.0.join("a.py"), &context()).unwrap();
    let saved = session.source(&symbols[0], &context()).unwrap();
    assert_eq!(saved.code, "alpha");
    assert_eq!(session.files(&context()).unwrap_err().kind, ErrorKind::Io);
    unavailable.store(false, Ordering::Release);
    assert_eq!(session.files(&context()).unwrap().len(), 2);
    unavailable.store(true, Ordering::Release);
    assert_eq!(
        session.search("sample", &context()).unwrap_err().kind,
        ErrorKind::Io
    );
    assert_eq!(saved.code, "alpha");
    session.dispose(&context()).unwrap();
}

#[test]
fn dependency_metadata_generation_invalidates_same_text_analyses_and_publishes_atomically() {
    let files = Files::new();
    let generation = Arc::new(AtomicU64::new(0));
    let prepared = prepare_provider(
        &files,
        "normal",
        Box::new(Provider {
            root: files.0.clone(),
            policy: SearchMergePolicy::DocumentsFirst,
            unavailable: Arc::new(AtomicBool::new(false)),
            generation: generation.clone(),
        }),
    )
    .unwrap();
    let mut session = prepared.session;
    session.search("sample", &context()).unwrap();
    assert_eq!(count(&files, "textDocument/documentSymbol"), 2);
    let initial = session.metadata_snapshot().unwrap();
    let symbols = session.symbols(&files.0.join("a.py"), &context()).unwrap();
    let saved = session.source(&symbols[0], &context()).unwrap();
    generation.store(1, Ordering::Release);
    session.files(&context()).unwrap();
    let refreshed = session.metadata_snapshot().unwrap();
    assert_eq!(refreshed.files, initial.files);
    assert!(initial.crates.is_empty());
    assert_eq!(refreshed.crates[0].name, "renamed-package");
    assert_eq!(refreshed.catalog_revision, initial.catalog_revision + 1);
    session.search("sample", &context()).unwrap();
    assert_eq!(count(&files, "textDocument/documentSymbol"), 4);
    assert_eq!(saved.code, "alpha");
    session.dispose(&context()).unwrap();
}

#[test]
fn initial_nonseed_content_read_failure_is_a_protected_catalog_outcome() {
    struct ReadFailure(Provider);
    impl MetadataProvider for ReadFailure {
        fn refresh(&mut self, context: &OperationContext) -> AnalysisResult<Metadata> {
            let mut metadata = self.0.refresh(context)?;
            metadata.files.push(self.0.root.join("unreadable.py"));
            Ok(metadata)
        }
    }
    let files = Files::new();
    let prepared = prepare_provider(
        &files,
        "normal",
        Box::new(ReadFailure(Provider {
            root: files.0.clone(),
            policy: SearchMergePolicy::DocumentsFirst,
            unavailable: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
        })),
    )
    .unwrap();
    assert!(
        matches!(prepared.catalog, CatalogOutcome::Failed(ref error) if error.kind == ErrorKind::Io)
    );
    let mut session = prepared.session;
    let symbols = session.symbols(&files.0.join("a.py"), &context()).unwrap();
    assert_eq!(
        session.source(&symbols[0], &context()).unwrap().code,
        "alpha"
    );
    assert_eq!(session.files(&context()).unwrap_err().kind, ErrorKind::Io);
    session.dispose(&context()).unwrap();
}

#[test]
fn common_navigation_preserves_link_metadata_and_existing_card_excerpt_policy() {
    use refscape_model::{Position, SourceRange};
    let files = Files::new();
    let prepared = prepare(&files, "normal", SearchMergePolicy::DocumentsFirst);
    let mut session = prepared.session;
    let path = files.0.join("a.py");
    let expected_symbol = session.symbols(&path, &context()).unwrap().remove(0);
    let targets = session
        .definitions(&path, Position::new(0, 1), &context())
        .unwrap();
    assert_eq!(targets.len(), 1);
    let target = &targets[0];
    assert_eq!(target.location.document, path);
    assert_eq!(
        target.location.target_range,
        SourceRange {
            start: Position::new(0, 0),
            end: Position::new(0, 5)
        }
    );
    assert_eq!(
        target.location.selection_range,
        SourceRange {
            start: Position::new(0, 0),
            end: Position::new(0, 1)
        }
    );
    assert_eq!(
        target.location.origin_range,
        Some(SourceRange {
            start: Position::new(0, 1),
            end: Position::new(0, 2)
        })
    );
    assert_eq!(target.symbol, expected_symbol);
    assert_eq!(
        session.source(&target.symbol, &context()).unwrap().code,
        "alpha"
    );
    session.dispose(&context()).unwrap();
}

#[test]
fn navigation_without_enclosing_symbol_keeps_selection_range_display_policy() {
    use refscape_model::Position;
    let files = Files::new();
    let prepared = prepare(&files, "null", SearchMergePolicy::DocumentsFirst);
    let mut session = prepared.session;
    let path = files.0.join("a.py");
    assert!(session.symbols(&path, &context()).unwrap().is_empty());
    let targets = session
        .definitions(&path, Position::new(0, 1), &context())
        .unwrap();
    assert_eq!(targets.len(), 1);
    let target = &targets[0];
    assert_eq!(target.location.document, path);
    assert_eq!(target.location.target_range.end, Position::new(0, 5));
    assert_eq!(target.location.selection_range.end, Position::new(0, 1));
    assert_eq!(target.symbol.range, target.location.selection_range);
    assert_eq!(
        target.symbol.selection_range,
        target.location.selection_range
    );
    assert_eq!(
        session.source(&target.symbol, &context()).unwrap().code,
        "a"
    );
    session.dispose(&context()).unwrap();
}
