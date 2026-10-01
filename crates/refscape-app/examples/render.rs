//! Render a real GPUI scene into a PNG for visual verification.
//! cargo run -p refscape-app --example render --features visual-tests -- PROJECT OUTPUT [light] [variable] [unfold] [arrange] [--compile-commands PATH]
use std::{env, path::PathBuf};

use refscape_application::explorer::Explorer;
use refscape_language::LanguageBackend;
use refscape_model::{Point, ProjectOptions, Theme};
use refscape_storage::session::JsonSessionRepository;

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
                    .expect("--compile-commands requires a path"),
            )
        });
    let variable = options.iter().any(|s| s == "variable");
    let theme = if options.iter().any(|s| s == "light") {
        Theme::light()
    } else {
        Theme::dark()
    };
    let mut explorer = Explorer::new(LanguageBackend::default(), JsonSessionRepository);
    explorer
        .open_project(
            &root,
            &ProjectOptions {
                compilation_database: database,
                ..Default::default()
            },
        )
        .unwrap();
    explorer.set_theme(theme).unwrap();
    // Document symbols are available without waiting for clangd's background index.
    let main = explorer
        .files()
        .unwrap()
        .into_iter()
        .find_map(|path| {
            explorer
                .symbols(&path)
                .unwrap()
                .into_iter()
                .find(|symbol| symbol.name == "main")
        })
        .expect("project must define main");
    let main_path = main.path.clone();
    let id = explorer.add_symbol(main, Point::new(30.0, 70.0)).unwrap();
    let card = explorer
        .session()
        .cards
        .iter()
        .find(|card| card.id == id)
        .unwrap();
    // Select an actual semantic function token within main, just as clicking the code does.
    let call = card
        .source
        .tokens
        .iter()
        .find(|token| {
            token.kind == if variable { "variable" } else { "function" }
                && token.line > card.source.symbol.selection_range.start.line
        })
        .map(|token| refscape_model::Position::new(token.line, token.start));
    if let Some(call) = call {
        if variable {
            explorer.toggle_type_definition(&id, call).unwrap();
        } else {
            explorer.expand_definition(&id, call).unwrap();
        }
    }
    if options.iter().any(|option| option == "arrange") && !variable {
        let source = &explorer
            .session()
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
            .map(|token| refscape_model::Position::new(token.line, token.start))
            .collect();
        for position in calls {
            explorer.expand_definition(&id, position).unwrap();
        }
    }
    if options.iter().any(|option| option == "unfold") {
        let gaps: Vec<_> = explorer
            .session()
            .cards
            .iter()
            .flat_map(|card| {
                (0..card.source.context.len())
                    .filter(|&index| card.source.folded_range(index).is_some())
                    .map(|index| (card.id.clone(), index))
            })
            .collect();
        for (id, index) in gaps {
            explorer.expand_context(&id, index).unwrap();
        }
    }
    if options.iter().any(|option| option == "arrange") {
        explorer.arrange_layout(Some(&id)).unwrap();
    }
    let cards = &explorer.session().cards;
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
    let zoom = (1080.0 / (right - left))
        .min(728.0 / (bottom - top))
        .clamp(0.65, 1.0);
    explorer.zoom(zoom, Point::default()).unwrap();
    explorer
        .pan(Point::new(40.0 - left * zoom, 50.0 - top * zoom))
        .unwrap();
    println!(
        "Rendering {} cards from {}",
        explorer.session().cards.len(),
        main_path.display()
    );
    refscape_ui::runtime::render_snapshot(
        explorer,
        root.join(".refscape/session.json"),
        output,
        if variable {
            call.map(|call| (id, call))
        } else {
            None
        },
    )
    .unwrap();
}
