use refscape_lsp::{
    LspProjectSession, ServerConfiguration,
    transport::{DefaultServerBehavior, ServerBehavior, Transport},
};
use refscape_model::{ErrorKind, FeatureResult, OperationContext, Position, SourceRange, Symbol};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
fn context() -> OperationContext {
    OperationContext::detached(Duration::from_secs(3))
}
fn command(scenario: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_refscape-lsp-fixture"));
    command.arg(scenario);
    command
}
fn transport(scenario: &str) -> Transport {
    Transport::spawn(
        &mut command(scenario),
        "fixture",
        "fixture missing",
        Box::new(DefaultServerBehavior),
    )
    .unwrap()
}
struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "refscape-lsp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("source.rs"), "sample\n").unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn source(&self) -> PathBuf {
        self.0.join("source.rs")
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.source());
        let _ = std::fs::remove_dir(&self.0);
    }
}
fn session(files: &Files, scenario: &str) -> LspProjectSession {
    let mut command = command(scenario);
    command.env("REFSCAPE_FIXTURE_PATH", files.source());
    session_command(files, command)
}
fn session_command(files: &Files, mut command: Command) -> LspProjectSession {
    LspProjectSession::start(
        files.0.clone(),
        &mut command,
        &context(),
        ServerConfiguration {
            name: "fixture".into(),
            installation_hint: "fixture".into(),
            initialization_options: Value::Null,
            experimental_capabilities: Value::Null,
            language_id: |_| "rust",
            behavior: Box::new(DefaultServerBehavior),
        },
    )
    .unwrap()
}
fn symbol(path: PathBuf) -> Symbol {
    Symbol {
        id: "sample".into(),
        name: "sample".into(),
        kind: "variable".into(),
        path,
        range: SourceRange {
            start: Position::new(0, 0),
            end: Position::new(0, 6),
        },
        selection_range: SourceRange {
            start: Position::new(0, 0),
            end: Position::new(0, 1),
        },
        children: vec![],
    }
}
#[test]
fn header_limit_and_eof_fail_without_unbounded_wait() {
    for scenario in ["header", "truncated", "exit"] {
        let client = transport(scenario);
        let disposal = client.disposal();
        let error = client
            .request("initialize", json!({}), &context())
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Protocol);
        drop(client);
        disposal.wait(&context()).unwrap();
    }
}
#[test]
fn cancellation_removes_pending_and_late_response_does_not_match_next_request() {
    let client = transport("normal");
    let ctx = context();
    let token = ctx.cancel.clone();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        token.cancel();
    });
    assert_eq!(
        client
            .request("test/late", Value::Null, &ctx)
            .unwrap_err()
            .kind,
        ErrorKind::Cancelled
    );
    canceller.join().unwrap();
    assert!(
        client
            .request("test/status", Value::Null, &context())
            .unwrap()
            .is_object()
    );
    let disposal = client.disposal();
    drop(client);
    disposal.wait(&context()).unwrap();
}
#[test]
fn blocked_stdin_write_obeys_deadline_and_process_is_reaped() {
    let client = transport("unread");
    let disposal = client.disposal();
    let start = Instant::now();
    let error = client
        .notify(
            "test/huge",
            json!({"data":"x".repeat(1024*1024)}),
            &OperationContext::detached(Duration::from_millis(150)),
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert!(start.elapsed() < Duration::from_secs(2));
    drop(client);
    disposal.wait(&context()).unwrap();
}
#[test]
fn unanswered_shutdown_is_supervised_and_drop_returns_immediately() {
    let client = transport("shutdown-stall");
    let disposal = client.disposal();
    let start = Instant::now();
    drop(client);
    assert!(start.elapsed() < Duration::from_millis(100));
    disposal.wait(&context()).unwrap();
}
#[test]
fn retries_share_one_operation_budget() {
    let client = transport("normal");
    assert_eq!(
        client
            .request("test/retry", Value::Null, &context())
            .unwrap(),
        "retried"
    );
    let short = OperationContext::detached(Duration::from_millis(20));
    assert_eq!(
        client
            .request("test/stall", Value::Null, &short)
            .unwrap_err()
            .kind,
        ErrorKind::Timeout
    );
}
struct Configuration;
impl ServerBehavior for Configuration {
    fn scoped_configuration(&self, section: Option<&str>, scope: Option<&str>) -> Value {
        json!({"section":section,"scope":scope})
    }
}
#[test]
fn idle_configuration_is_serviced_even_after_notification_flood() {
    for scenario in ["idle", "flood"] {
        let client = Transport::spawn(
            &mut command(scenario),
            "fixture",
            "fixture",
            Box::new(Configuration),
        )
        .unwrap();
        client.notify("initialized", json!({}), &context()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let status = client
                .request("test/status", Value::Null, &context())
                .unwrap();
            if !status["idle"].is_null() {
                assert_eq!(
                    status["idle"],
                    json!([{"section":"test","scope":"file:///scope"}])
                );
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
#[test]
fn source_symbols_and_tokens_use_the_same_snapshot_when_file_changes_during_rpc() {
    let files = Files::new();
    let mut session = session(&files, "mutate");
    let source = session.source(&symbol(files.source()), &context()).unwrap();
    assert_eq!(source.code, "sample");
    assert_eq!(source.tokens.len(), 1);
    assert_eq!(
        std::fs::read_to_string(files.source()).unwrap(),
        "changed unrelated text\n"
    );
}
#[test]
fn symbols_then_source_capture_one_file_for_the_entire_job() {
    let files = Files::new();
    let mut session = session(&files, "mutate");
    let job = context();
    let symbols = session.symbols(&files.source(), &job).unwrap();
    assert_eq!(session.source(&symbols[0], &job).unwrap().code, "sample");
    assert_eq!(session.document_statistics().disk_reads, 1);
    assert_eq!(session.document_statistics().operation_snapshots, 1);
    session.finish_operation(&job);
    assert_eq!(session.document_statistics().operation_snapshots, 0);
    let mut whole = symbols[0].clone();
    whole.kind = "file".into();
    assert_eq!(
        session.source(&whole, &context()).unwrap().code,
        "changed unrelated text\n"
    );
    assert_eq!(session.document_statistics().disk_reads, 2);
}
#[test]
fn highlights_then_hover_and_repeated_locations_share_the_job_snapshot() {
    let files = Files::new();
    let mut inspection = session(&files, "inspection-mutate");
    let job = context();
    inspection
        .document_highlights(&files.source(), Position::new(0, 0), &job)
        .unwrap();
    assert_eq!(
        inspection
            .hover(&files.source(), Position::new(0, 0), &job)
            .unwrap(),
        FeatureResult::Supported(Some("sample\n".into()))
    );
    assert_eq!(inspection.document_statistics().disk_reads, 1);
    assert_eq!(
        inspection
            .hover(&files.source(), Position::new(0, 0), &context())
            .unwrap(),
        FeatureResult::Supported(Some("changed unrelated text\n".into()))
    );
    std::fs::write(files.source(), "sample\n").unwrap();
    let mut navigation = session(&files, "repeated-location");
    let job = context();
    let targets = navigation
        .references(&files.source(), Position::new(0, 0), &job)
        .unwrap();
    assert_eq!(targets.len(), 1);
    std::fs::write(files.source(), "changed unrelated text\n").unwrap();
    assert_eq!(
        navigation.source(&targets[0].symbol, &job).unwrap().code,
        "sample"
    );
    assert_eq!(navigation.document_statistics().disk_reads, 1);
}
#[test]
fn malformed_encoding_and_location_link_ranges_are_protocol_errors() {
    let files = Files::new();
    let error = LspProjectSession::start(
        files.0.clone(),
        &mut command("invalid-encoding"),
        &context(),
        ServerConfiguration {
            name: "fixture".into(),
            installation_hint: "fixture".into(),
            initialization_options: Value::Null,
            experimental_capabilities: Value::Null,
            language_id: |_| "rust",
            behavior: Box::new(DefaultServerBehavior),
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Protocol);
    let error = LspProjectSession::start(
        files.0.clone(),
        &mut command("invalid-provider"),
        &context(),
        ServerConfiguration {
            name: "fixture".into(),
            installation_hint: "fixture".into(),
            initialization_options: Value::Null,
            experimental_capabilities: Value::Null,
            language_id: |_| "rust",
            behavior: Box::new(DefaultServerBehavior),
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Protocol);
    for scenario in ["invalid-hover", "invalid-highlight", "invalid-token"] {
        let mut session = session(&files, scenario);
        let error = match scenario {
            "invalid-hover" => session
                .hover(&files.source(), Position::new(0, 0), &context())
                .unwrap_err(),
            "invalid-highlight" => session
                .document_highlights(&files.source(), Position::new(0, 0), &context())
                .unwrap_err(),
            _ => session
                .source(&symbol(files.source()), &context())
                .unwrap_err(),
        };
        assert_eq!(error.kind, ErrorKind::Protocol);
    }
    for scenario in ["invalid-target", "invalid-origin", "invalid-selection"] {
        let mut session = session(&files, scenario);
        assert_eq!(
            session
                .navigation_locations(
                    &files.source(),
                    Position::new(0, 0),
                    "textDocument/definition",
                    &context()
                )
                .unwrap_err()
                .kind,
            ErrorKind::Protocol
        );
        assert_eq!(
            session
                .definitions(&files.source(), Position::new(0, 0), &context())
                .unwrap_err()
                .kind,
            ErrorKind::Protocol
        );
    }
}
#[test]
fn refresh_before_late_tokens_invalidates_results_and_never_caches_them() {
    let files = Files::new();
    let mut session = session(&files, "refresh");
    assert_eq!(
        session
            .source(&symbol(files.source()), &context())
            .unwrap()
            .tokens
            .len(),
        1
    );
    assert_eq!(session.analysis_epoch(), 1);
    assert_eq!(
        session
            .source(&symbol(files.source()), &context())
            .unwrap()
            .tokens
            .len(),
        1
    );
}
#[test]
fn range_only_tokens_are_unsupported_and_other_features_remain_available() {
    let files = Files::new();
    let mut session = session(&files, "range");
    assert!(!session.capabilities().semantic_tokens);
    assert!(
        session
            .source(&symbol(files.source()), &context())
            .unwrap()
            .tokens
            .is_empty()
    );
    assert_eq!(
        session
            .hover(&files.source(), Position::new(0, 0), &context())
            .unwrap(),
        FeatureResult::Supported(Some("hover".into()))
    );
    assert_eq!(
        session
            .definitions(&files.source(), Position::new(0, 0), &context())
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn optional_unsupported_empty_and_malformed_are_distinct() {
    let files = Files::new();
    let mut unsupported = session(&files, "empty");
    assert_eq!(
        unsupported
            .hover(&files.source(), Position::new(0, 0), &context())
            .unwrap(),
        FeatureResult::Unsupported
    );
    let mut empty = session(&files, "null");
    assert!(
        empty
            .symbols(&files.source(), &context())
            .unwrap()
            .is_empty()
    );
    let mut malformed = session(&files, "malformed");
    assert_eq!(
        malformed
            .symbols(&files.source(), &context())
            .unwrap_err()
            .kind,
        ErrorKind::Protocol
    );
}
#[test]
fn location_links_preserve_target_selection_and_origin_ranges() {
    let files = Files::new();
    let mut session = session(&files, "normal");
    let locations = session
        .navigation_locations(
            &files.source(),
            Position::new(0, 0),
            "textDocument/definition",
            &context(),
        )
        .unwrap();
    assert_eq!(locations[0].target_range.end.character, 6);
    assert_eq!(locations[0].document, files.source());
    assert_eq!(locations[0].selection_range.end.character, 1);
    assert_eq!(locations[0].origin_range.unwrap().start.character, 1);
    let targets = session
        .definitions(&files.source(), Position::new(0, 0), &context())
        .unwrap();
    assert_eq!(targets[0].location, locations[0]);
    assert_eq!(targets[0].symbol.range.end.character, 6);
}
#[test]
fn metadata_epoch_rebuilds_symbols_and_tokens_without_reopening_or_reading_the_job() {
    let files = Files::new();
    let log = files.0.join("rpc.jsonl");
    let mut command = command("normal");
    command.env("REFSCAPE_FIXTURE_LOG", &log);
    let mut session = session_command(&files, command);
    let job = context();
    session.source(&symbol(files.source()), &job).unwrap();
    assert_eq!(session.invalidate_analysis(), 1);
    session.source(&symbol(files.source()), &job).unwrap();
    let messages = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let count = |method: &str| {
        messages
            .iter()
            .filter(|message| message["method"] == method)
            .count()
    };
    assert_eq!(count("textDocument/documentSymbol"), 2);
    assert_eq!(count("textDocument/semanticTokens/full"), 2);
    assert_eq!(count("textDocument/didOpen"), 1);
    assert_eq!(count("textDocument/didChange"), 0);
    assert_eq!(session.document_statistics().disk_reads, 1);
    let disposal = session.disposal();
    drop(session);
    disposal.wait(&context()).unwrap();
    std::fs::remove_file(log).unwrap();
}
#[test]
fn dynamic_registration_updates_capabilities_and_legend_before_acknowledgment() {
    let files = Files::new();
    let mut session = session(&files, "dynamic");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !session.capabilities().semantic_tokens {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(session.capabilities().document_symbols);
    assert_eq!(session.analysis_epoch(), 1);
    let source = session.source(&symbol(files.source()), &context()).unwrap();
    assert_eq!(source.tokens[0].kind, "property");
}
#[test]
fn workspace_folders_and_read_only_edit_responses_keep_their_contracts() {
    for scenario in ["folders", "apply"] {
        let client = transport(scenario);
        let folders = json!([{"uri":"file:///project","name":"project"}]);
        client.set_folders(folders.clone(), &context()).unwrap();
        client.notify("initialized", json!({}), &context()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let status = client
                .request("test/status", Value::Null, &context())
                .unwrap();
            if !status["idle"].is_null() {
                if scenario == "folders" {
                    assert_eq!(status["idle"], folders);
                } else {
                    assert_eq!(status["idle"]["applied"], false);
                }
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
#[test]
fn process_runner_cancels_and_reaps_metadata_on_shared_runtime() {
    let error = refscape_lsp::runtime::run_process(
        &mut command("unread"),
        &OperationContext::detached(Duration::from_millis(50)),
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Timeout);
    let output = refscape_lsp::runtime::run_process(&mut command("exit"), &context()).unwrap();
    assert!(!output.success);
}
#[test]
fn rpc_diagnostics_record_deadlines_and_notification_memory_is_bounded() {
    let client = transport("flood");
    client.notify("initialized", json!({}), &context()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let status = client
            .request("test/status", Value::Null, &context())
            .unwrap();
        if !status["idle"].is_null() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(client.recent_notifications().len(), 128);
    let ctx = OperationContext::detached(Duration::from_millis(20));
    let error = client.request("test/stall", Value::Null, &ctx).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert_eq!(error.method.as_deref(), Some("test/stall"));
    let diagnostics = client.diagnostics();
    assert!(diagnostics.len() <= 128);
    assert_eq!(
        diagnostics.last().unwrap().outcome,
        Some(ErrorKind::Timeout)
    );
}
#[test]
fn document_cache_eviction_keeps_server_residency_until_catalog_removal() {
    let files = Files::new();
    let mut session = session(&files, "normal");
    let mut paths = vec![];
    for index in 0..130 {
        let path = files.0.join(format!("bulk-{index}.rs"));
        std::fs::write(&path, "sample\n").unwrap();
        assert_eq!(session.symbols(&path, &context()).unwrap().len(), 1);
        paths.push(path.canonicalize().unwrap());
    }
    let stats = session.document_statistics();
    assert_eq!(stats.opened, 130);
    assert_eq!(stats.snapshots, 128);
    assert!(stats.symbol_caches <= 128);
    session.close_documents(&paths, &context()).unwrap();
    assert_eq!(session.document_statistics().opened, 0);
    for path in paths {
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn local_document_cache_obeys_byte_budget_as_well_as_entry_limit() {
    let files = Files::new();
    let mut session = session(&files, "normal");
    let mut paths = vec![];
    for index in 0..20 {
        let path = files.0.join(format!("large-{index}.rs"));
        std::fs::write(&path, "x".repeat(128 * 1024)).unwrap();
        session.symbols(&path, &context()).unwrap();
        paths.push(path.canonicalize().unwrap());
    }
    let stats = session.document_statistics();
    assert_eq!(stats.opened, 20);
    assert!(stats.snapshots < 20);
    assert!(stats.retained_bytes <= 32 * 1024 * 1024);
    session.close_documents(&paths, &context()).unwrap();
    for path in paths {
        std::fs::remove_file(path).unwrap();
    }
}
