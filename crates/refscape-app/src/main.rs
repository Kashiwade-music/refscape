//! Composition boundary: one environment snapshot, controller and concrete worker ports.
mod check;
mod launch;
mod options;
use launch::{LaunchConfig, LaunchRequest};
use options::{HELP, Options};
use refscape_application::{ApplicationController, Command, SessionRepository, WorkerExecutor};
use refscape_language::{EnvironmentSnapshot, LanguageBackend};
use refscape_model::Theme;
use refscape_storage::{
    session::{JsonSessionRepository, default_session_path},
    theme::{load_theme, save_theme},
};
use std::{env, path::PathBuf, process::ExitCode, sync::Arc};
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Refscape: {error}");
            ExitCode::FAILURE
        }
    }
}
fn backend(config: &LaunchConfig) -> LanguageBackend {
    let mut backend = LanguageBackend::from_environment(EnvironmentSnapshot::capture());
    if let Some(path) = &config.analyzer {
        backend = backend.with_rust_analyzer(path);
    }
    if let Some(path) = &config.clangd {
        backend = backend.with_clangd(path);
    }
    if let Some(path) = &config.typescript {
        backend = backend.with_typescript_server(path);
    }
    if let Some(path) = &config.pyright {
        backend = backend.with_pyright_server(path);
    }
    backend
}
fn run() -> Result<(), String> {
    match Options::parse(env::args_os().skip(1))?.into_request()? {
        LaunchRequest::Help => {
            print!("{HELP}");
            Ok(())
        }
        LaunchRequest::ExportTheme { name, path } => {
            let theme = if name == "light" {
                Theme::light()
            } else {
                Theme::dark()
            };
            save_theme(&path, &theme)?;
            println!("Saved {} theme to {}", theme.name, path.display());
            Ok(())
        }
        LaunchRequest::Check { root, config } => {
            check::project(backend(&config), &root, &config.project_options)
        }
        LaunchRequest::Gui(config) => gui(config),
    }
}
fn canonical(path: PathBuf) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|error| format!("cannot open {}: {error}", path.display()))
}
fn gui(config: LaunchConfig) -> Result<(), String> {
    let factory = backend(&config);
    let mut controller = ApplicationController::new();
    let mut themes = Vec::new();
    if let Some(path) = &config.theme {
        let theme = load_theme(path)?;
        let transition = controller.dispatch(Command::SetTheme(theme.clone()));
        if transition.events.iter().any(|event| {
            matches!(
                event,
                refscape_application::ViewEvent::Status { error: true, .. }
            )
        }) {
            return Err(controller.status().into());
        }
        themes.push(theme);
    }
    // Decode exactly once; startup and worker preparation share these fixed bytes.
    let loaded = config
        .session
        .as_ref()
        .filter(|path| path.exists())
        .map(|path| JsonSessionRepository.load(path))
        .transpose()
        .map_err(|error| error.to_string())?;
    let project = config.project.map(canonical).transpose()?;
    if let (Some(loaded), Some(project)) = (&loaded, &project)
        && canonical(loaded.snapshot.project_root.clone())? != *project
    {
        return Err(
            "the selected session belongs to a different project; omit PROJECT to open its project"
                .into(),
        );
    }
    let root = loaded
        .as_ref()
        .map(|loaded| loaded.snapshot.project_root.clone())
        .or(project.clone());
    let session_path = config.session.clone().unwrap_or_else(|| {
        root.as_deref()
            .map(default_session_path)
            .unwrap_or_default()
    });
    let initial = if let Some(loaded) = loaded {
        Some(Command::OpenLoaded {
            loaded,
            destination: session_path.clone(),
            expected_root: project,
            overrides: config.project_options.clone(),
        })
    } else {
        root.map(|root| Command::OpenProject {
            root,
            options: config.project_options.clone(),
            destination: session_path.clone(),
        })
    };
    let executor = Arc::new(WorkerExecutor::new(
        Arc::new(factory),
        Arc::new(JsonSessionRepository),
    ));
    refscape_ui::runtime::run(
        controller,
        executor,
        session_path,
        themes,
        initial,
        config.project_options,
    )
}
