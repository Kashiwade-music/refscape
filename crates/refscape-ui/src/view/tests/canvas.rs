use super::layout::assert_layout;
use super::*;

fn assert_code_columns(card: &PaintedCard, zoom: f32) {
    for row in &card.rows {
        if let Some(number) = &row.number {
            let right = number.origin.x + number.line.x_for_index(number.line.text.len());
            assert!((f32::from(right - (card.origin.x - px(12.0 * zoom)))).abs() < 0.01);
            assert!(number.origin.x >= card.bounds.left() + px(24.0 * zoom));
        }
    }
}

#[gpui::test]
fn folded_context_keeps_clicks_and_connections_on_the_original_source_row(cx: &mut TestAppContext) {
    let (explorer, requests) = fixture();
    let mut session = explorer.snapshot().clone();
    let mut source = session.cards[0].source.to_document();
    source.context.push(refscape_model::SourceContext {
        start_line: 0,
        code: "impl Sample {".into(),
    });
    Arc::make_mut(&mut session.cards)[0].source = source.try_into().unwrap();
    let source_id = session.cards[0].id.clone();
    let mut target = session.cards[0].clone();
    target.id = "target".into();
    target.position = Point::new(800.0, 100.0).try_into().unwrap();
    Arc::make_mut(&mut session.cards).push(target);
    Arc::make_mut(&mut session.connections).push(refscape_model::Connection {
        id: "edge".into(),
        from: source_id,
        to: "target".into(),
        kind: ConnectionKind::Definition,
        source: Position::new(12, 9),
    });
    let explorer = import_fixture(explorer, session.clone());
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::from_fixture(
            explorer,
            "session.json".into(),
            vec![],
            None,
            ProjectOpenOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let (bounds, origin, x, y) = view.read_with(cx, |view, _| {
        let painted = &view.canvas.painted[0];
        assert_eq!(
            painted
                .rows
                .iter()
                .map(|row| row.position)
                .collect::<Vec<_>>(),
            vec![Some(Position::new(0, 0)), None, Some(Position::new(12, 5))]
        );
        assert_eq!(painted.rows[0].code.text.as_ref(), "impl Sample {");
        assert_eq!(
            painted.rows[1].code.text.as_ref(),
            "    ... (Show 11 Lines)"
        );
        (
            view.canvas.bounds,
            painted.origin,
            painted.origin.x + painted.rows[2].code.x_for_index("日本😀".len()) + px(1.0),
            painted.origin.y + px(2.0 * LINE + 5.0),
        )
    });
    cx.update_window(handle, |_, window, _| {
        let links = code_connections(
            &session,
            bounds,
            &mut super::super::scene::SceneCache::default(),
            window,
        );
        assert_eq!(links.len(), 1);
        assert!(links[0].start.y > origin.y + px(2.0 * LINE));
        assert!(links[0].start.y < origin.y + px(3.0 * LINE));
    })
    .unwrap();
    // The synthetic destination above is only needed to verify painted connection geometry.
    view.update(cx, |view, cx| {
        view.command(
            Command::CloseCard {
                id: "target".into(),
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    cx.simulate_mouse_down(
        point(x, origin.y + px(LINE + 5.0)),
        MouseButton::Right,
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(requests.lock().unwrap().is_empty());
    cx.simulate_mouse_down(point(x, y), MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.requests.error, "{}", view.requests.status)
    });
    assert_eq!(*requests.lock().unwrap(), vec![Position::new(12, 9)]);
}

#[gpui::test]
fn context_symbols_and_revealed_lines_are_clickable_at_absolute_utf16_positions(
    cx: &mut TestAppContext,
) {
    let range = SourceRange {
        start: Position::new(12, 5),
        end: Position::new(12, 13),
    };
    let source = SourceDocument {
        expanded: Vec::new(),
        symbol: Symbol::file("sample.rs".into(), range),
        code: "日本😀call".into(),
        tokens: vec![],
        code_start: None,
        context: vec![refscape_model::SourceContext {
            start_line: 9,
            code: "impl 日本😀Sample {".into(),
        }],
        folded: vec![refscape_model::SourceContext {
            start_line: 10,
            code: "    call();\n\n".into(),
        }],
    };
    let mut target = Symbol::file("target.rs".into(), range);
    target.name = "Sample".into();
    let (explorer, requests) = source_fixture(source, vec![target]);
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::from_fixture(
            explorer,
            "session.json".into(),
            vec![],
            None,
            ProjectOpenOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let click = view.read_with(cx, |view, _| {
        let card = &view.canvas.painted[0];
        assert_code_columns(card, view.controller.snapshot().viewport.zoom);
        assert_eq!(card.rows[1].code.text.as_ref(), "    ... (Show 2 Lines)");
        point(
            card.origin.x + card.rows[0].code.x_for_index("impl 日本😀".len()) + px(1.0),
            card.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_move(click, None, Modifiers::default());
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    assert_eq!(*requests.lock().unwrap(), vec![Position::new(9, 9)]);
    requests.lock().unwrap().clear();
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(*requests.lock().unwrap(), vec![Position::new(9, 9)]);
    view.read_with(cx, |view, _| {
        assert!(!view.requests.error, "{}", view.requests.status);
        assert_eq!(view.controller.snapshot().cards.len(), 2);
        assert_eq!(
            view.controller.snapshot().connections[0].source,
            Position::new(9, 9)
        );
    });
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let (session, bounds, origin) = view.read_with(cx, |view, _| {
        (
            view.controller.snapshot().clone(),
            view.canvas.bounds,
            view.canvas.painted[0].origin,
        )
    });
    cx.update_window(handle, |_, window, _| {
        let links = code_connections(
            &session,
            bounds,
            &mut super::super::scene::SceneCache::default(),
            window,
        );
        assert_eq!(links.len(), 1);
        assert!(links[0].start.y > origin.y && links[0].start.y < origin.y + px(LINE));
    })
    .unwrap();
    let fold = view.read_with(cx, |view, _| {
        let card = &view.canvas.painted[0];
        point(card.origin.x + px(5.0), card.origin.y + px(LINE + 5.0))
    });
    cx.simulate_mouse_down(fold, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.requests.error, "{}", view.requests.status);
        let card = &view.controller.snapshot().cards[0];
        assert!(card.source.export_folded().is_empty());
        assert_eq!(card.source.display_row(Position::new(12, 9)), Some(3));
        assert_eq!(card.source.display_lines()[1].text, "    call();");
        assert_eq!(card.source.display_lines()[2].text, "");
        view.controller.snapshot().validate().unwrap();
    });
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let click = view.read_with(cx, |view, _| {
        let card = &view.canvas.painted[0];
        assert_code_columns(card, view.controller.snapshot().viewport.zoom);
        point(
            card.origin.x + card.rows[1].code.x_for_index(4) + px(1.0),
            card.origin.y + px(LINE + 5.0),
        )
    });
    cx.simulate_mouse_down(click, MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        *requests.lock().unwrap(),
        vec![Position::new(9, 9), Position::new(10, 4)]
    );
    for zoom in [0.75, 1.5] {
        view.update(cx, |view, cx| {
            view.command(
                Command::SetViewport(Viewport {
                    zoom,
                    ..view.controller.snapshot().viewport
                }),
                cx,
            );
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let number_click = view.read_with(cx, |view, _| {
            let card = &view.canvas.painted[0];
            assert_code_columns(card, zoom);
            point(
                card.rows[1].number.as_ref().unwrap().origin.x + px(1.0),
                card.origin.y + px((LINE + 5.0) * zoom),
            )
        });
        cx.simulate_mouse_move(number_click, None, Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| assert!(view.canvas.context_hover.is_none()));
        cx.simulate_mouse_down(number_click, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.controller.snapshot().cards[0]
                    .source
                    .export_expanded()
                    .len(),
                1
            )
        });
        let collapse = view.read_with(cx, |view, _| {
            let card = &view.canvas.painted[0];
            assert_eq!(card.rows[1].fold, Some(0));
            point(
                card.bounds.left() + px(10.0 * zoom),
                card.origin.y + px((LINE + 5.0) * zoom),
            )
        });
        cx.simulate_mouse_move(collapse, None, Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| assert!(view.canvas.context_hover.is_some()));
        cx.simulate_mouse_down(collapse, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(!view.requests.error, "{}", view.requests.status);
            assert!(
                view.controller.snapshot().cards[0]
                    .source
                    .export_expanded()
                    .is_empty()
            );
            assert_eq!(
                view.controller.snapshot().cards[0]
                    .source
                    .display_row(Position::new(12, 9)),
                Some(2)
            );
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let (session, bounds, expand) = view.read_with(cx, |view, _| {
            let card = &view.canvas.painted[0];
            assert_code_columns(card, zoom);
            assert_eq!(card.rows[1].code.text.as_ref(), "    ... (Show 2 Lines)");
            (
                view.controller.snapshot().clone(),
                view.canvas.bounds,
                point(
                    card.bounds.left() + px(10.0 * zoom),
                    card.origin.y + px((LINE + 5.0) * zoom),
                ),
            )
        });
        cx.update_window(handle, |_, window, _| {
            assert_eq!(
                code_connections(
                    &session,
                    bounds,
                    &mut super::super::scene::SceneCache::default(),
                    window
                )
                .len(),
                1
            )
        })
        .unwrap();
        cx.simulate_mouse_down(expand, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(!view.requests.error, "{}", view.requests.status);
            assert_eq!(
                view.controller.snapshot().cards[0]
                    .source
                    .export_expanded()
                    .len(),
                1
            );
            assert_eq!(
                view.controller.snapshot().cards[0]
                    .source
                    .display_row(Position::new(12, 9)),
                Some(3)
            );
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let (session, bounds) = view.read_with(cx, |view, _| {
            (view.controller.snapshot().clone(), view.canvas.bounds)
        });
        cx.update_window(handle, |_, window, _| {
            assert_eq!(
                code_connections(
                    &session,
                    bounds,
                    &mut super::super::scene::SceneCache::default(),
                    window
                )
                .len(),
                2
            )
        })
        .unwrap();
    }
    assert_eq!(requests.lock().unwrap().len(), 2);
}
#[gpui::test]
fn hiding_a_card_keeps_survivor_positions_and_updates_connection_anchors(cx: &mut TestAppContext) {
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
    let viewport = explorer.snapshot().viewport;
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::from_fixture(
            explorer,
            "session.json".into(),
            vec![],
            None,
            ProjectOpenOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let click = view.read_with(cx, |view, _| {
        let source = &view.canvas.painted[0];
        point(
            source.origin.x + source.rows[0].code.x_for_index("日本😀".len()) + px(1.0),
            source.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    let before = view.read_with(cx, |view, _| {
        view.controller
            .snapshot()
            .cards
            .iter()
            .map(|card| (card.id.clone(), card.position))
            .collect::<Vec<_>>()
    });
    view.update(cx, |view, cx| view.toggle_symbol(first, cx));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let (session, bounds) = view.read_with(cx, |view, _| {
        assert!(!view.requests.error, "{}", view.requests.status);
        assert_eq!(view.controller.snapshot().cards.len(), 3);
        let target = view
            .controller
            .snapshot()
            .cards
            .iter()
            .find(|card| card.source.symbol.path == second.path)
            .unwrap();
        assert_eq!(
            target.position,
            before.iter().find(|(id, _)| id == &target.id).unwrap().1
        );
        assert_eq!(
            view.controller
                .snapshot()
                .cards
                .iter()
                .find(|card| card.id == far)
                .unwrap()
                .position,
            before.iter().find(|(id, _)| id == &far).unwrap().1
        );
        assert_eq!(view.controller.snapshot().viewport, viewport);
        view.controller.snapshot().validate().unwrap();
        (view.controller.snapshot().clone(), view.canvas.bounds)
    });
    cx.update_window(handle, |_, window, _| {
        let links = code_connections(
            &session,
            bounds,
            &mut super::super::scene::SceneCache::default(),
            window,
        );
        assert_eq!(links.len(), 1);
        let target = session
            .cards
            .iter()
            .find(|card| card.source.symbol.path == second.path)
            .unwrap();
        let target_bounds = super::super::painting::card_bounds(target, &session, bounds);
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
    let mut session = explorer.snapshot().clone();
    let mut source = session.cards[0].source.to_document();
    source.tokens.push(refscape_model::SemanticToken {
        line: 12,
        start: 9,
        length: 4,
        kind: "function".into(),
        modifiers: vec![],
    });
    Arc::make_mut(&mut session.cards)[0].source = source.try_into().unwrap();
    let mut target = session.cards[0].clone();
    target.id = "target".into();
    target.position = Point::new(800.0, 100.0).try_into().unwrap();
    Arc::make_mut(&mut session.cards).push(target);
    for (index, kind) in [
        refscape_model::ConnectionKind::Definition,
        refscape_model::ConnectionKind::Reference,
    ]
    .into_iter()
    .enumerate()
    {
        Arc::make_mut(&mut session.connections).push(refscape_model::Connection {
            id: format!("edge-{index}").into(),
            from: session.cards[0].id.clone(),
            to: "target".into(),
            kind,
            source: Position::new(12, 11),
        });
    }
    let explorer = import_fixture(explorer, session.clone());
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::from_fixture(
            explorer,
            PathBuf::from("session.json"),
            vec![],
            None,
            ProjectOpenOptions::default(),
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
        session.viewport.offset = offset.try_into().unwrap();
        view.update(cx, |view, cx| {
            view.command(Command::SetViewport(session.viewport), cx);
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let (bounds, word_start, word_end) = view.read_with(cx, |view, _| {
            let card = &view.canvas.painted[0];
            (
                view.canvas.bounds,
                card.origin.x + card.rows[0].code.x_for_index("日本😀".len()),
                card.origin.x + card.rows[0].code.x_for_index("日本😀call".len()),
            )
        });
        cx.update_window(handle, |_, window, _| {
            let edges = code_connections(
                &session,
                bounds,
                &mut super::super::scene::SceneCache::default(),
                window,
            );
            assert_eq!(edges.len(), 2);
            for edge in edges {
                assert!(f32::from(edge.underline.left() - word_start).abs() < 0.001);
                assert!(f32::from(edge.underline.right() - word_end).abs() < 0.001);
                assert!(f32::from(edge.start.x - word_end).abs() < 0.001);
                assert!(
                    edge.start.x
                        < super::super::painting::card_bounds(&session.cards[0], &session, bounds)
                            .right()
                );
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
    let mut source = session.cards[0].source.to_document();
    source.tokens.clear();
    Arc::make_mut(&mut session.cards)[0].source = source.try_into().unwrap();
    assert_eq!(
        connected_word(&session.cards[0], Position::new(12, 11))
            .unwrap()
            .1,
        "日本😀".len().."日本😀call".len()
    );
    session.viewport.zoom = 0.5;
    cx.update_window(handle, |_, window, _| {
        assert!(
            code_connections(
                &session,
                Bounds::default(),
                &mut super::super::scene::SceneCache::default(),
                window
            )
            .is_empty()
        );
    })
    .unwrap();
}

#[gpui::test]
fn dragging_tall_cards_previews_then_places_only_the_target_at_every_zoom(cx: &mut TestAppContext) {
    let (initial, _) = fixture();
    let mut source = initial.snapshot().cards[0].source.to_document();
    source.code = std::iter::repeat_n("fn source() {}", 12)
        .collect::<Vec<_>>()
        .join("\n");
    let (mut explorer, _) = source_fixture(source, vec![]);
    let first = explorer.snapshot().cards[0].id.clone();
    explorer.move_card(&first, Point::new(20.0, 10.0)).unwrap();
    let range = explorer.snapshot().cards[0].source.symbol.range;
    let second = explorer
        .add_symbol(
            Symbol::file("second.rs".into(), range),
            Point::new(20.0, 450.0),
        )
        .unwrap();
    explorer
        .add_symbol(
            Symbol::file("third.rs".into(), range),
            Point::new(20.0, 900.0),
        )
        .unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::from_fixture(
            explorer,
            PathBuf::from("session.json"),
            vec![],
            None,
            ProjectOpenOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let handle = cx.window_handle();
    for zoom in [0.75, 1.0, 1.5] {
        view.update(cx, |view, cx| {
            view.command(
                Command::MoveCard {
                    id: second.clone(),
                    position: Point::new(20.0, 450.0),
                },
                cx,
            )
        });
        cx.run_until_parked();
        view.update(cx, |view, cx| {
            view.command(
                Command::SetViewport(Viewport {
                    zoom,
                    offset: Point::new(13.0, 27.0).try_into().unwrap(),
                }),
                cx,
            );
            view.canvas.drag = Some(Drag::Card(
                second.clone(),
                point(px(0.0), px(0.0)),
                Point::new(20.0, 450.0),
            ));
            let fixed = [
                view.controller.snapshot().cards[0].position,
                view.controller.snapshot().cards[2].position,
            ];
            view.mouse_move(
                &MouseMoveEvent {
                    position: point(px(0.0), px(-440.0 * zoom)),
                    ..Default::default()
                },
                cx,
            );
            assert_eq!(
                view.controller.snapshot().cards[1].position,
                Point::new(20.0, 450.0)
            );
            assert_eq!(
                view.canvas.drag_preview,
                Some((second.clone(), Point::new(20.0, 10.0)))
            );
            view.finish_drag(cx);
            assert_eq!(
                [
                    view.controller.snapshot().cards[0].position,
                    view.controller.snapshot().cards[2].position
                ],
                fixed
            );
        });
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        view.read_with(cx, |view, _| {
            assert!(view.canvas.drag.is_none());
            assert!(view.canvas.drag_preview.is_none());
            assert_eq!(
                view.controller.snapshot().cards[0].position,
                Point::new(20.0, 10.0)
            );
            assert_eq!(
                view.controller.snapshot().cards[2].position,
                Point::new(20.0, 900.0)
            );
            assert_layout(view.controller.snapshot());
            for painted in &view.canvas.painted {
                let card = view
                    .controller
                    .snapshot()
                    .cards
                    .iter()
                    .find(|card| card.id == painted.id)
                    .unwrap();
                let total_rows = card.source.projection().rows.len();
                assert!(painted.first_row + painted.rows.len() <= total_rows);
                let last_line_bottom = painted.origin.y + px(total_rows as f32 * LINE * zoom);
                assert!(painted.bounds.bottom() >= last_line_bottom + px(16.0 * zoom));
                assert_eq!(painted.bounds.size.height, px(card.display_height() * zoom));
            }
        });
    }
}

#[gpui::test]
fn cards_moved_during_a_request_place_new_cards_from_the_latest_parent(cx: &mut TestAppContext) {
    let target = Symbol::file(
        "target.rs".into(),
        SourceRange {
            start: Position::new(12, 5),
            end: Position::new(12, 13),
        },
    );
    let (explorer, requests) = fixture_with_targets(vec![target]);
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::from_fixture(
            explorer,
            "session.json".into(),
            vec![],
            None,
            ProjectOpenOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        let parent = view.controller.snapshot().cards[0].id.clone();
        view.command(
            Command::Navigate {
                card: parent.to_string(),
                position: Position::new(12, 9),
                kind: ConnectionKind::Definition,
                anchor: Point::new(90.0, HEADER + 8.0),
                toggle: true,
            },
            cx,
        );
        // The worker planned against the old position before the pointer moved.
        view.canvas.drag = Some(Drag::Card(
            view.controller.snapshot().cards[0].id.to_string(),
            point(px(0.0), px(0.0)),
            view.controller.snapshot().cards[0].position.point(),
        ));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(700.0), px(0.0)),
                ..Default::default()
            },
            cx,
        );
        assert_eq!(
            view.controller.snapshot().cards[0].position,
            Point::new(100.0, 50.0)
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.controller.snapshot().cards.len(), 1);
        assert!(view.controller.dragging());
        assert_eq!(
            view.controller.snapshot().cards[0].position,
            Point::new(100.0, 50.0)
        );
        assert!(view.requests.busy);
    });
    view.update(cx, |view, cx| view.finish_drag(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.requests.error);
        assert_eq!(view.controller.snapshot().cards.len(), 2);
        assert_eq!(
            view.controller.snapshot().cards[0].position,
            Point::new(800.0, 50.0)
        );
        let parent = &view.controller.snapshot().cards[0];
        assert_eq!(
            view.controller.snapshot().cards[1].position,
            Point::new(
                parent.position.x + parent.width + 100.0,
                parent.position.y + HEADER + 8.0
            )
        );
        assert_layout(view.controller.snapshot());
    });
    assert_eq!(*requests.lock().unwrap(), vec![Position::new(12, 9)]);
    // A failed read-only request leaves the committed positions exactly intact.
    let before = view.read_with(cx, |view, _| view.controller.snapshot().cards.clone());
    view.update(cx, |view, cx| {
        view.search(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.requests.error);
        assert_eq!(view.controller.snapshot().cards, before);
    });
}

fn import_fixture(fixture: FixtureDriver, snapshot: ApplicationSnapshot) -> FixtureDriver {
    let (_, executor) = fixture.driver.into_parts();
    let mut loaded = FixtureDriver {
        driver: refscape_application::HeadlessDriver::new(executor),
    };
    loaded
        .dispatch(Command::OpenLoaded {
            loaded: ImportedSession { snapshot },
            destination: "session.json".into(),
            expected_root: None,
            overrides: ProjectOpenOptions::default(),
        })
        .unwrap();
    loaded
}
