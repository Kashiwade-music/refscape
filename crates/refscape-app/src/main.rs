//! Composition root: native UI, Rust/C/C++ analysis, and versioned JSON storage.

mod options;

use std::{
    env,
    path::Path,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

use refscape_application::{explorer::Explorer, ports::SessionRepository};
use refscape_language::LanguageBackend;
use refscape_model::{Point, ProjectOptions, Theme};
use refscape_storage::{
    session::{JsonSessionRepository, default_session_path},
    theme::{load_theme, save_theme},
};

use options::{HELP, Options};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Refscape: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut options = Options::parse(env::args_os().skip(1))?;
    if options.help {
        print!("{HELP}");
        return Ok(());
    }
    if let Some((name, path)) = options.export_theme {
        let theme = if name == "light" {
            Theme::light()
        } else {
            Theme::dark()
        };
        save_theme(&path, &theme)?;
        println!("Saved {} theme to {}", theme.name, path.display());
        return Ok(());
    }
    let analyzer = options
        .analyzer
        .or_else(|| env::var_os("REFSCAPE_RUST_ANALYZER").map(Into::into))
        .unwrap_or_else(|| "rust-analyzer".into());
    let clangd = options
        .clangd
        .or_else(|| env::var_os("REFSCAPE_CLANGD").map(Into::into))
        .unwrap_or_else(|| "clangd".into());
    let mut explorer = Explorer::new(
        LanguageBackend::new(analyzer, clangd),
        JsonSessionRepository,
    );
    if options.check {
        return check_project(
            &mut explorer,
            options
                .project
                .as_deref()
                .ok_or("--check requires a project")?,
            &options.project_options,
        );
    }
    let mut themes = Vec::new();
    if let Some(path) = options.theme {
        let theme = load_theme(&path)?;
        explorer.set_theme(theme.clone())?;
        themes.push(theme);
    }
    if let Some(path) = options.session.as_ref().filter(|path| path.exists()) {
        let session = JsonSessionRepository.load(path)?;
        if let Some(project) = &options.project {
            let project = std::fs::canonicalize(project)
                .map_err(|e| format!("cannot open {}: {e}", project.display()))?;
            let saved = std::fs::canonicalize(&session.project_root).map_err(|e| e.to_string())?;
            if project != saved {
                return Err("the selected session belongs to a different project; omit PROJECT to open its project".into());
            }
        }
        options.project = Some(session.project_root);
    }
    let project = options
        .project
        .map(|path| {
            std::fs::canonicalize(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))
        })
        .transpose()?;
    let session_path = options.session.unwrap_or_else(|| {
        project
            .as_deref()
            .map(default_session_path)
            .unwrap_or_default()
    });
    refscape_ui::runtime::run(
        explorer,
        session_path,
        themes,
        project,
        options.project_options,
    )
}

/// Real-backend smoke check, with a disposable session that never overwrites user work.
fn check_project(
    explorer: &mut Explorer<LanguageBackend, JsonSessionRepository>,
    project: &Path,
    project_options: &ProjectOptions,
) -> Result<(), String> {
    let project = std::fs::canonicalize(project).map_err(|e| e.to_string())?;
    explorer.open_project(&project, project_options)?;
    let files = explorer.files()?;
    let mut selected = None;
    for path in &files {
        if let Some(symbol) = explorer.symbols(path)?.into_iter().next() {
            selected = Some(symbol);
            break;
        }
    }
    let symbol = selected.ok_or("project contains no symbols")?;
    let position = symbol.selection_range.start;
    let id = explorer.add_symbol(symbol, Point::new(40.0, 40.0))?;
    let card = explorer
        .session()
        .cards
        .iter()
        .find(|card| card.id == id)
        .ok_or("source card was not created")?;
    if card.source.code.is_empty() {
        return Err("language backend returned empty source".into());
    }
    let token_count = card.source.tokens.len();
    let definitions = explorer.expand_definition(&id, position)?.len();
    let references = explorer.expand_references(&id, position)?.len();
    explorer.move_card(&id, Point::new(120.0, 80.0))?;
    explorer.pan(Point::new(-20.0, 35.0))?;
    explorer.zoom(0.8, Point::new(200.0, 200.0))?;
    let snapshot = explorer.session().clone();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = env::temp_dir().join(format!(
        "refscape-check-{}-{stamp}.json",
        std::process::id()
    ));
    let result: Result<(), String> = (|| {
        explorer.save_session(&path)?;
        explorer.load_session(&path)?;
        if explorer.session() != &snapshot {
            return Err("saved session did not restore exactly".into());
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(path);
    result?;
    println!(
        "Refscape check passed: {} source files, {} cards, {} connections; \
         {definitions} definition results, {references} reference results, \
         {token_count} semantic tokens; canvas and session roundtrip verified",
        files.len(),
        snapshot.cards.len(),
        snapshot.connections.len()
    );
    Ok(())
}
