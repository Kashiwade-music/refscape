//! Headless validation uses the same controller commands as the native view.
use refscape_application::{Command, HeadlessDriver, ViewEvent, WorkerExecutor};
use refscape_language::LanguageBackend;
use refscape_model::{ConnectionKind, Point, ProjectOpenOptions};
use refscape_storage::session::JsonSessionRepository;
use std::{
    env,
    path::Path,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
fn dispatch(driver: &mut HeadlessDriver, command: Command) -> Result<Vec<ViewEvent>, String> {
    let events = driver.dispatch(command);
    if let Some(message) = events.iter().find_map(|event| match event {
        ViewEvent::Status {
            message,
            error: true,
        } => Some(message.clone()),
        _ => None,
    }) {
        return Err(message);
    }
    Ok(events)
}
fn targets(events: Vec<ViewEvent>) -> Vec<String> {
    events
        .into_iter()
        .find_map(|event| match event {
            ViewEvent::Canvas(outcome) => Some(outcome.targets),
            _ => None,
        })
        .unwrap_or_default()
}
pub fn project(
    factory: LanguageBackend,
    root: &Path,
    options: &ProjectOpenOptions,
) -> Result<(), String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let path = env::temp_dir().join(format!(
        "refscape-check-{}-{stamp}.json",
        std::process::id()
    ));
    let mut driver = HeadlessDriver::new(Arc::new(WorkerExecutor::new(
        Arc::new(factory),
        Arc::new(JsonSessionRepository),
    )));
    let result = (|| {
        let events = dispatch(
            &mut driver,
            Command::OpenProject {
                root,
                options: options.clone(),
                destination: path.clone(),
            },
        )?;
        let files = events
            .into_iter()
            .find_map(|event| match event {
                ViewEvent::Files(files) => Some(files),
                _ => None,
            })
            .unwrap_or_default();
        let mut selected = None;
        for file in &files {
            let events = dispatch(&mut driver, Command::Symbols(file.clone()))?;
            selected = events.into_iter().find_map(|event| match event {
                ViewEvent::Symbols(symbols) => symbols.into_iter().next(),
                _ => None,
            });
            if selected.is_some() {
                break;
            }
        }
        let symbol = selected.ok_or("project contains no symbols")?;
        let position = symbol.selection_range.start;
        let id = targets(dispatch(
            &mut driver,
            Command::AddSymbol {
                symbol,
                position: Point::new(40., 40.),
                toggle: false,
            },
        )?)
        .into_iter()
        .next()
        .ok_or("source card was not created")?;
        let card = driver
            .controller
            .snapshot()
            .cards
            .iter()
            .find(|card| card.id == id)
            .ok_or("source card was not created")?;
        if card.source.code.is_empty() {
            return Err("language backend returned empty source".into());
        }
        let token_count = card.source.tokens.len();
        let definitions = targets(dispatch(
            &mut driver,
            Command::Navigate {
                card: id.clone(),
                position,
                kind: ConnectionKind::Definition,
                anchor: Point::default(),
                toggle: false,
            },
        )?)
        .len();
        let references = targets(dispatch(
            &mut driver,
            Command::Navigate {
                card: id.clone(),
                position,
                kind: ConnectionKind::Reference,
                anchor: Point::default(),
                toggle: false,
            },
        )?)
        .len();
        dispatch(
            &mut driver,
            Command::MoveCard {
                id,
                position: Point::new(120., 80.),
            },
        )?;
        dispatch(&mut driver, Command::Pan(Point::new(-20., 35.)))?;
        dispatch(
            &mut driver,
            Command::Zoom {
                factor: 0.8,
                anchor: Point::new(200., 200.),
            },
        )?;
        let snapshot = driver.controller.shared_snapshot();
        dispatch(&mut driver, Command::Save)?;
        dispatch(
            &mut driver,
            Command::OpenSession {
                path: path.clone(),
                expected_root: None,
                overrides: options.clone(),
            },
        )?;
        if driver.controller.snapshot() != snapshot.as_ref() {
            return Err("saved session did not restore exactly".into());
        }
        println!(
            "Refscape check passed: {} source files, {} cards, {} connections; {definitions} definition results, {references} reference results, {token_count} semantic tokens; canvas and session roundtrip verified",
            files.len(),
            snapshot.cards.len(),
            snapshot.connections.len()
        );
        Ok(())
    })();
    let _ = std::fs::remove_file(path);
    result
}
