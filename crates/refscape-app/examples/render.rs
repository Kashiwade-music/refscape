//! Capture a real native GPUI scene after draw readiness.
use refscape_application::{Command, HeadlessDriver, ViewEvent, WorkerExecutor};
use refscape_language::LanguageBackend;
use refscape_model::{
    CODE_CARD_HEADER, CODE_LINE_HEIGHT, ConnectionKind, Point, Position, ProjectOpenOptions, Theme,
};
use refscape_storage::session::JsonSessionRepository;
use std::{env, path::PathBuf, sync::Arc};
fn run(driver: &mut HeadlessDriver, command: Command) -> Vec<ViewEvent> {
    let events = driver.dispatch(command);
    if let Some(message) = events.iter().find_map(|event| match event {
        ViewEvent::Status {
            message,
            error: true,
        } => Some(message),
        _ => None,
    }) {
        panic!("{message}");
    }
    events
}
fn navigate(
    driver: &mut HeadlessDriver,
    id: &str,
    position: Position,
    kind: ConnectionKind,
    toggle: bool,
) {
    let card = driver
        .controller
        .snapshot()
        .cards
        .iter()
        .find(|card| card.id == id)
        .unwrap();
    let anchor = Point::new(
        card.width,
        CODE_CARD_HEADER
            + 8.
            + card.source.display_anchor_row(position).unwrap_or(0) as f32 * CODE_LINE_HEIGHT,
    );
    run(
        driver,
        Command::Navigate {
            card: id.into(),
            position,
            kind,
            anchor,
            toggle,
        },
    );
}
fn main() {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(args.next().expect("PROJECT required"))
        .canonicalize()
        .unwrap();
    let output = PathBuf::from(args.next().expect("OUTPUT required"));
    let options: Vec<_> = args.collect();
    let database = options
        .iter()
        .position(|arg| arg == "--compile-commands")
        .map(|index| {
            PathBuf::from(
                options
                    .get(index + 1)
                    .expect("--compile-commands requires path"),
            )
        });
    let variable = options.iter().any(|arg| arg == "variable");
    let theme = if options.iter().any(|arg| arg == "light") {
        Theme::light()
    } else {
        Theme::dark()
    };
    let executor = Arc::new(WorkerExecutor::new(
        Arc::new(LanguageBackend::default()),
        Arc::new(JsonSessionRepository),
    ));
    let mut driver = HeadlessDriver::new(executor.clone());
    // Capture never saves or restores a user's default project session.
    let scratch = root.join(".refscape/session.json");
    let events = run(
        &mut driver,
        Command::OpenLoaded {
            loaded: refscape_application::ImportedSession {
                snapshot: refscape_application::ApplicationSnapshot::new(root.clone()),
            },
            destination: scratch,
            expected_root: Some(root.clone()),
            overrides: ProjectOpenOptions {
                compilation_database: database,
                ..Default::default()
            },
        },
    );
    run(&mut driver, Command::SetTheme(theme));
    let files = events
        .into_iter()
        .find_map(|event| match event {
            ViewEvent::Files(files) => Some(files),
            _ => None,
        })
        .unwrap();
    let main = files
        .into_iter()
        .find_map(|path| {
            run(&mut driver, Command::Symbols(path))
                .into_iter()
                .find_map(|event| match event {
                    ViewEvent::Symbols(symbols) => {
                        symbols.into_iter().find(|symbol| symbol.name == "main")
                    }
                    _ => None,
                })
        })
        .expect("project must define main");
    let main_path = main.path.clone();
    let id = run(
        &mut driver,
        Command::AddSymbol {
            symbol: main,
            position: Point::new(30., 70.),
            toggle: false,
        },
    )
    .into_iter()
    .find_map(|event| match event {
        ViewEvent::Canvas(outcome) => outcome.targets.into_iter().next(),
        _ => None,
    })
    .unwrap();
    let card = driver
        .controller
        .snapshot()
        .cards
        .iter()
        .find(|card| card.id == id)
        .unwrap();
    let call = card
        .source
        .tokens
        .iter()
        .find(|token| {
            token.kind == if variable { "variable" } else { "function" }
                && token.line > card.source.symbol.selection_range.start.line
        })
        .map(|token| Position::new(token.line, token.start));
    if let Some(call) = call {
        navigate(
            &mut driver,
            &id,
            call,
            if variable {
                ConnectionKind::TypeDefinition
            } else {
                ConnectionKind::Definition
            },
            variable,
        );
    }
    if options.iter().any(|arg| arg == "arrange") && !variable {
        let source = &driver
            .controller
            .snapshot()
            .cards
            .iter()
            .find(|card| card.id == id)
            .unwrap()
            .source;
        let calls: Vec<_> = source
            .tokens
            .iter()
            .filter(|token| {
                token.kind == "function" && token.line > source.symbol.selection_range.start.line
            })
            .map(|token| Position::new(token.line, token.start))
            .collect();
        for position in calls {
            navigate(
                &mut driver,
                &id,
                position,
                ConnectionKind::Definition,
                false,
            );
        }
    }
    if options.iter().any(|arg| arg == "unfold") {
        let gaps: Vec<_> = driver
            .controller
            .snapshot()
            .cards
            .iter()
            .flat_map(|card| {
                (0..card.source.context.len())
                    .filter(|index| card.source.folded_range(*index).is_some())
                    .map(|index| (card.id.to_string(), index))
            })
            .collect();
        for (card, index) in gaps {
            run(
                &mut driver,
                Command::ToggleFold {
                    card,
                    index,
                    expand: true,
                },
            );
        }
    }
    if options.iter().any(|arg| arg == "arrange") {
        run(
            &mut driver,
            Command::Arrange {
                selected: Some(id.clone()),
            },
        );
    }
    let cards = &driver.controller.snapshot().cards;
    let left = cards
        .iter()
        .map(|card| card.position.x)
        .fold(f32::INFINITY, f32::min);
    let top = cards
        .iter()
        .map(|card| card.position.y)
        .fold(f32::INFINITY, f32::min);
    let right = cards
        .iter()
        .map(|card| card.position.x + card.width)
        .fold(f32::NEG_INFINITY, f32::max);
    let bottom = cards
        .iter()
        .map(|card| card.position.y + card.height)
        .fold(f32::NEG_INFINITY, f32::max);
    let zoom = (1080. / (right - left))
        .min(728. / (bottom - top))
        .clamp(0.65, 1.);
    run(
        &mut driver,
        Command::Zoom {
            factor: zoom,
            anchor: Point::default(),
        },
    );
    run(
        &mut driver,
        Command::Pan(Point::new(40. - left * zoom, 50. - top * zoom)),
    );
    println!(
        "Rendering {} cards from {}",
        driver.controller.snapshot().cards.len(),
        main_path.display()
    );
    let (controller, executor) = driver.into_parts();
    refscape_ui::runtime::render_snapshot(
        controller,
        executor,
        root.join(".refscape/session.json"),
        output,
        if variable {
            call.map(|position| (id, position))
        } else {
            None
        },
    )
    .unwrap();
}
