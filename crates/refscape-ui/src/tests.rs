use super::*;
use gpui::{Modifiers, TestAppContext, VisualContext};
use refscape_model::{SourceDocument, SourceRange};
use std::path::Path;

struct Language {
    source: SourceDocument,
    requests: Arc<Mutex<Vec<Position>>>,
}
impl LanguageService for Language {
    fn open_project(&mut self, _: &Path) -> Result<(), String> {
        Ok(())
    }
    fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        Ok(vec![])
    }
    fn symbols(&mut self, _: &Path) -> Result<Vec<Symbol>, String> {
        Ok(vec![])
    }
    fn source(&mut self, _: &Symbol) -> Result<SourceDocument, String> {
        Ok(self.source.clone())
    }
    fn definitions(&mut self, _: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.requests.lock().unwrap().push(position);
        Ok(vec![])
    }
    fn references(&mut self, _: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.requests.lock().unwrap().push(position);
        Ok(vec![])
    }
    fn search(&mut self, _: &str) -> Result<Vec<Symbol>, String> {
        Err("simulated analyzer failure".into())
    }
}
struct Repository;
impl SessionRepository for Repository {
    fn save(&self, _: &Path, _: &Session) -> Result<(), String> {
        Err("simulated storage failure".into())
    }
    fn load(&self, _: &Path) -> Result<Session, String> {
        Err("invalid fixture session".into())
    }
}
fn fixture() -> (Explorer<Language, Repository>, Arc<Mutex<Vec<Position>>>) {
    let range = SourceRange {
        start: Position::new(12, 5),
        end: Position::new(12, 13),
    };
    let symbol = Symbol::file(PathBuf::from("sample.rs"), range);
    let source = SourceDocument {
        symbol: symbol.clone(),
        code: "日本😀call".into(),
        tokens: vec![],
    };
    let requests = Arc::new(Mutex::new(vec![]));
    let mut explorer = Explorer::new(
        Language {
            source,
            requests: requests.clone(),
        },
        Repository,
    );
    explorer
        .add_symbol(symbol, Point::new(100.0, 50.0))
        .unwrap();
    (explorer, requests)
}

#[gpui::test]
fn native_source_click_uses_shaped_glyphs_and_absolute_utf16_positions(cx: &mut TestAppContext) {
    let (explorer, requests) = fixture();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            PathBuf::from("session.json"),
            vec![],
            None,
            window,
            cx,
        )
    });
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let click = view.read_with(cx, |view, _| {
        let card = &view.painted[0];
        point(
            card.origin.x + card.lines[0].x_for_index("日本😀".len()) + px(1.0),
            card.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(*requests.lock().unwrap(), vec![Position::new(12, 9)]);
    cx.simulate_mouse_down(click, MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[gpui::test]
fn asynchronous_failure_keeps_canvas_movement_and_pointer_zoom_anchor(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            PathBuf::from("session.json"),
            vec![],
            None,
            window,
            cx,
        )
    });
    view.update(cx, |view, cx| {
        view.search(cx);
        let anchor = Point::new(400.0, 200.0);
        let before = view.session.viewport.screen_to_world(anchor);
        view.zoom(1.5, anchor, cx);
        assert_eq!(before, view.session.viewport.screen_to_world(anchor));
        view.drag = Some(Drag::Pan(point(px(0.0), px(0.0))));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(35.0), px(60.0)),
                ..Default::default()
            },
            cx,
        );
        view.drag = Some(Drag::Card(
            view.session.cards[0].id.clone(),
            point(px(0.0), px(0.0)),
            Point::new(100.0, 50.0),
        ));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(150.0), px(75.0)),
                ..Default::default()
            },
            cx,
        );
    });
    let moved = view.read_with(cx, |view, _| {
        (view.session.viewport, view.session.cards[0].position)
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.error);
        assert_eq!(view.status, "simulated analyzer failure");
        assert_eq!(
            (view.session.viewport, view.session.cards[0].position),
            moved
        );
    });
}

#[gpui::test]
fn portable_custom_theme_survives_cycle_and_close_waits_for_requests(cx: &mut TestAppContext) {
    let (mut explorer, _) = fixture();
    let mut custom = Theme::dark();
    custom.palette.accent = "#FF0000".into();
    explorer.set_theme(custom.clone()).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            PathBuf::from("session.json"),
            vec![Theme::dark(), custom.clone()],
            None,
            window,
            cx,
        )
    });
    view.update_in(cx, |view, window, cx| {
        assert_eq!(view.themes.len(), 3);
        view.busy = true;
        assert!(!view.close(window, cx));
        assert!(!view.closing);
        view.busy = false;
        view.cycle_theme(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.session.theme, Theme::dark()));
    view.update(cx, |view, cx| view.cycle_theme(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.session.theme, Theme::light()));
    view.update(cx, |view, cx| view.cycle_theme(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.session.theme, custom));
}

#[gpui::test]
fn definition_and_reference_edges_start_at_rendered_word_underlines(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
    let mut session = explorer.session().clone();
    session.cards[0]
        .source
        .tokens
        .push(refscape_model::SemanticToken {
            line: 12,
            start: 9,
            length: 4,
            kind: "function".into(),
            modifiers: vec![],
        });
    let mut target = session.cards[0].clone();
    target.id = "target".into();
    target.position = Point::new(800.0, 100.0);
    session.cards.push(target);
    for (index, kind) in [
        refscape_model::ConnectionKind::Definition,
        refscape_model::ConnectionKind::Reference,
    ]
    .into_iter()
    .enumerate()
    {
        session.connections.push(refscape_model::Connection {
            id: format!("edge-{index}"),
            from: session.cards[0].id.clone(),
            to: "target".into(),
            kind,
            source: Position::new(12, 11),
        });
    }
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            PathBuf::from("session.json"),
            vec![],
            None,
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let handle = cx.window_handle();
    for (zoom, offset) in [
        (1.0, Point::default()),
        (0.75, Point::new(30.0, 45.0)),
        (1.5, Point::new(-40.0, 10.0)),
    ] {
        session.viewport.zoom = zoom;
        session.viewport.offset = offset;
        view.update(cx, |view, cx| {
            view.session = session.clone();
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let (bounds, word_start, word_end) = view.read_with(cx, |view, _| {
            let card = &view.painted[0];
            (
                view.bounds,
                card.origin.x + card.lines[0].x_for_index("日本😀".len()),
                card.origin.x + card.lines[0].x_for_index("日本😀call".len()),
            )
        });
        cx.update_window(handle, |_, window, _| {
            let edges = code_connections(&session, bounds, window);
            assert_eq!(edges.len(), 2);
            for edge in edges {
                assert!(f32::from(edge.underline.left() - word_start).abs() < 0.001);
                assert!(f32::from(edge.underline.right() - word_end).abs() < 0.001);
                assert!(f32::from(edge.start.x - word_end).abs() < 0.001);
                assert!(edge.start.x < card_bounds(&session.cards[0], &session, bounds).right());
                let painted_bounds = edge.underline.scale(window.scale_factor());
                assert!(
                    window.painted_quads().iter().any(|quad| {
                        // GPUI snaps filled rectangles to physical pixel edges.
                        (quad.bounds.left().0 - painted_bounds.left().0).abs() <= 0.51
                            && (quad.bounds.right().0 - painted_bounds.right().0).abs() <= 0.51
                            && (quad.bounds.top().0 - painted_bounds.top().0).abs() <= 0.51
                            && (quad.bounds.bottom().0 - painted_bounds.bottom().0).abs() <= 0.51
                    }),
                    "the linked word must actually be underlined in the rendered scene"
                );
            }
        })
        .unwrap();
    }
    // A restored snapshot without semantic tokens still attaches to text, and
    // abstract zoom levels never fall back to a card-edge attachment.
    session.cards[0].source.tokens.clear();
    assert_eq!(
        connected_word(&session.cards[0], Position::new(12, 11))
            .unwrap()
            .1,
        "日本😀".len().."日本😀call".len()
    );
    session.viewport.zoom = 0.5;
    cx.update_window(handle, |_, window, _| {
        assert!(code_connections(&session, Bounds::default(), window).is_empty());
    })
    .unwrap();
}
