//! Render a real GPUI scene into a PNG for visual verification.
//! cargo run -p refscape-app --example render --features visual-tests -- PROJECT OUTPUT [light]
use std::{env, path::PathBuf, time::Duration};

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use refscape_application::Explorer;
use refscape_language::RustAnalyzer;
use refscape_model::{Point, Theme};
use refscape_storage::JsonSessionRepository;
use refscape_ui::ExplorerView;

fn main() {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(args.next().expect("PROJECT required"))
        .canonicalize()
        .unwrap();
    let output = PathBuf::from(args.next().expect("OUTPUT required"));
    let theme = if args.next().is_some_and(|s| s == "light") {
        Theme::light()
    } else {
        Theme::dark()
    };
    let mut explorer = Explorer::new(RustAnalyzer::default(), JsonSessionRepository);
    explorer.open_project(&root).unwrap();
    explorer.set_theme(theme).unwrap();
    let symbols = explorer.search("main").unwrap();
    let main = symbols
        .into_iter()
        .find(|s| s.name == "main")
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
            token.kind == "function" && token.line > card.source.symbol.selection_range.start.line
        })
        .map(|token| refscape_model::Position::new(token.line, token.start));
    if let Some(call) = call {
        explorer.expand_definition(&id, call).unwrap();
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
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    cx.new(|cx| {
                        ExplorerView::new(
                            explorer,
                            root.join(".refscape/session.json"),
                            vec![],
                            None,
                            window,
                            cx,
                        )
                    })
                },
            )
            .unwrap();
        cx.update_window(handle.into(), |_, window, _| {
            window.resize(size(px(1440.), px(900.)))
        })
        .unwrap();
        cx.spawn(async move |cx| {
            // Let the native resize event initialize the DirectX render target.
            cx.background_executor()
                .timer(Duration::from_millis(800))
                .await;
            cx.update_window(handle.into(), |_, window, cx| {
                let arena = window.draw(cx);
                let image = window.render_to_image().expect("GPU rendering failed");
                assert!(
                    image.width() >= 1000 && image.height() >= 600,
                    "native window has not resized"
                );
                image.save(&output).expect("PNG write failed");
                arena.clear(cx);
                println!("Saved {}", output.display());
            })
            .unwrap();
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
}
