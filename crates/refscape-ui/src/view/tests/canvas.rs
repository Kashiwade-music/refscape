use super::*;
#[gpui::test]
fn hiding_a_card_applies_the_new_layout_to_the_canvas_and_connection_anchors(
    cx: &mut TestAppContext,
) {
    let range = SourceRange {
        start: Position::new(12, 5),
        end: Position::new(12, 13),
    };
    let first = Symbol::file("first.rs".into(), range);
    let second = Symbol::file("second.rs".into(), range);
    let (mut explorer, _) = fixture_with_targets(vec![first.clone(), second.clone()]);
    let far = explorer
        .add_symbol(
            Symbol::file("far.rs".into(), range),
            Point::new(1960.0, 50.0),
        )
        .unwrap();
    explorer.zoom(1.25, Point::default()).unwrap();
    explorer.pan(Point::new(-50.0, 20.0)).unwrap();
    let viewport = explorer.session().viewport;
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            "session.json".into(),
            vec![],
            None,
            ProjectOptions::default(),
            window,
            cx,
        )
    });
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let click = view.read_with(cx, |view, _| {
        let source = &view.canvas.painted[0];
        point(
            source.origin.x + source.lines[0].x_for_index("日本😀".len()) + px(1.0),
            source.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    view.update(cx, |view, cx| view.toggle_symbol(first, cx));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let (session, bounds) = view.read_with(cx, |view, _| {
        assert!(!view.requests.error, "{}", view.requests.status);
        assert_eq!(view.session.cards.len(), 3);
        let target = view
            .session
            .cards
            .iter()
            .find(|card| card.source.symbol.path == second.path)
            .unwrap();
        assert_eq!(target.position, Point::new(720.0, 50.0 + HEADER + 8.0));
        assert_eq!(
            view.session
                .cards
                .iter()
                .find(|card| card.id == far)
                .unwrap()
                .position,
            Point::new(1340.0, 50.0)
        );
        assert_eq!(view.session.viewport, viewport);
        assert_eq!(
            view.session.cards,
            view.explorer.lock().unwrap().session().cards
        );
        (view.session.clone(), view.canvas.bounds)
    });
    cx.update_window(handle, |_, window, _| {
        let links = code_connections(&session, bounds, window);
        assert_eq!(links.len(), 1);
        let target = session
            .cards
            .iter()
            .find(|card| card.source.symbol.path == second.path)
            .unwrap();
        let target_bounds = card_bounds(target, &session, bounds);
        assert_eq!(
            links[0].end,
            point(
                target_bounds.left(),
                target_bounds.top() + px(HEADER * viewport.zoom * 0.5)
            )
        );
    })
    .unwrap();
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
            ProjectOptions::default(),
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
            let card = &view.canvas.painted[0];
            (
                view.canvas.bounds,
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

#[gpui::test]
fn dropping_tall_cards_clears_their_rendered_bottoms_at_every_zoom(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            PathBuf::from("session.json"),
            vec![],
            None,
            ProjectOptions::default(),
            window,
            cx,
        )
    });
    let handle = cx.window_handle();
    for zoom in [0.75, 1.0, 1.5] {
        view.update(cx, |view, cx| {
            let mut first = view.session.cards[0].clone();
            first.source.code = std::iter::repeat_n("fn source() {}", 12)
                .collect::<Vec<_>>()
                .join("\n");
            first.height = 128.0;
            first.position = Point::new(20.0, 10.0);
            let mut second = first.clone();
            second.id = "second".into();
            second.position.y = 170.0;
            let mut third = first.clone();
            third.id = "third".into();
            third.position.y = 330.0;
            view.session.cards = vec![first, second, third];
            view.session.viewport.zoom = zoom;
            view.session.viewport.offset = Point::new(13.0, 27.0);
            view.canvas.drag = Some(Drag::Card(
                "second".into(),
                point(px(0.0), px(0.0)),
                Point::new(20.0, 170.0),
            ));
            view.finish_drag(cx);
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        view.read_with(cx, |view, _| {
            assert!(view.canvas.drag.is_none());
            let rects: Vec<_> = view
                .session
                .cards
                .iter()
                .map(|card| card_bounds(card, &view.session, view.canvas.bounds))
                .collect();
            for pair in rects.windows(2) {
                assert!(f32::from(pair[1].top() - pair[0].bottom()) >= 32.0 * zoom - 0.001);
            }
            for painted in &view.canvas.painted {
                let last_line_bottom =
                    painted.origin.y + px(painted.lines.len() as f32 * LINE * zoom);
                assert!(painted.bounds.bottom() >= last_line_bottom + px(16.0 * zoom));
                let card = view
                    .session
                    .cards
                    .iter()
                    .find(|card| card.id == painted.id)
                    .unwrap();
                assert_eq!(painted.bounds.size.height, px(card.height * zoom));
            }
        });
    }
}

#[gpui::test]
fn cards_moved_during_a_request_do_not_overlap_new_cards_on_completion(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
    let mut target = explorer.session().cards[0].source.symbol.clone();
    target.id = "new-target".into();
    target.path = "target.rs".into();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            "session.json".into(),
            vec![],
            None,
            ProjectOptions::default(),
            window,
            cx,
        )
    });
    view.update(cx, |view, cx| {
        view.run_job(
            "Opening target",
            Box::new(move |explorer| {
                explorer.add_symbol(target, Point::new(800.0, 50.0))?;
                Ok(Output::default())
            }),
            cx,
        );
        // The worker planned against the old position before the pointer moved.
        view.canvas.drag = Some(Drag::Card(
            view.session.cards[0].id.clone(),
            point(px(0.0), px(0.0)),
            view.session.cards[0].position,
        ));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(700.0), px(0.0)),
                ..Default::default()
            },
            cx,
        );
        view.finish_drag(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.requests.error);
        assert_eq!(view.session.cards.len(), 2);
        assert_eq!(view.session.cards[0].position, Point::new(800.0, 50.0));
        let source = card_bounds(&view.session.cards[0], &view.session, view.canvas.bounds);
        let target = card_bounds(&view.session.cards[1], &view.session, view.canvas.bounds);
        assert!(f32::from(target.top() - source.bottom()) >= 32.0);
    });
    // A failing request also merges the current UI positions, then clears overlap.
    view.update(cx, |view, cx| {
        view.search(cx);
        view.session.cards[0].position = view.session.cards[1].position;
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.requests.error);
        let source = card_bounds(&view.session.cards[0], &view.session, view.canvas.bounds);
        let target = card_bounds(&view.session.cards[1], &view.session, view.canvas.bounds);
        assert!(f32::from(target.top() - source.bottom()) >= 32.0);
    });
}
