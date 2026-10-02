//! Deterministic renderer work bounds, separate from native GPU image validation.
use super::*;

fn long_source(lines: u32) -> SourceDocument {
    SourceDocument {
        symbol: refscape_model::Symbol::file(
            "long.rs".into(),
            SourceRange {
                start: Position::new(0, 0),
                end: Position::new(lines - 1, 4),
            },
        ),
        code: std::iter::repeat_n("call", lines as usize)
            .collect::<Vec<_>>()
            .join("\n"),
        tokens: vec![],
        context: vec![],
        code_start: None,
        expanded: vec![],
        folded: vec![],
    }
}

#[gpui::test]
fn long_card_work_tracks_visible_rows_and_cached_summary_metrics_at_zoom_boundaries(
    cx: &mut TestAppContext,
) {
    let (fixture, _) = source_fixture(long_source(20_000), vec![]);
    let source = fixture.snapshot().cards[0].source.snapshot().clone();
    let projection = fixture.snapshot().cards[0].source.shared_projection();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::from_fixture(
            fixture,
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
    for zoom in [0.349_999, 0.35, 0.649_999, 0.65, 1.0] {
        view.update(cx, |view, cx| {
            view.command(
                Command::SetViewport(Viewport {
                    zoom,
                    ..Viewport::default()
                }),
                cx,
            );
        });
        cx.run_until_parked();
        let before = view.update(cx, |view, cx| {
            view.scene.borrow_mut().clear();
            cx.notify();
            view.scene.borrow().shaped_rows
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        view.read_with(cx, |view, _| {
            let cache = view.scene.borrow();
            let work = cache.frame_work;
            let visible: usize = view.canvas.painted.iter().map(|card| card.rows.len()).sum();
            assert!(work.shape_misses <= work.visible_rows + work.edge_rows);
            assert_eq!(work.visible_rows, visible);
            assert_eq!(work.edge_rows, 0);
            assert_eq!(work.painted_edges, 0);
            if zoom < 0.35 {
                assert!(view.canvas.painted.is_empty());
                assert_eq!(work.summary_metric_reads, 0);
            } else if zoom < 0.65 {
                assert_eq!(work.summary_metric_reads, 1);
                assert_eq!(visible, 0);
                assert_eq!(cache.shaped_rows, before);
            } else {
                assert_eq!(work.summary_metric_reads, 0);
                assert!(visible > 0);
                let screen_rows =
                    (f32::from(view.canvas.bounds.size.height) / (LINE * zoom)).ceil() as usize;
                assert!(visible <= screen_rows + 1);
                assert!(visible < 200, "20,000 source rows must not all be visited");
                assert_eq!(cache.shaped_rows - before, visible as u64);
            }
            let card = &view.controller.snapshot().cards[0];
            assert_eq!(card.source.body_line_count(), 20_000);
            assert!(Arc::ptr_eq(card.source.snapshot(), &source));
            assert!(Arc::ptr_eq(&card.source.shared_projection(), &projection));
        });
    }
    let id = view.read_with(cx, |view, _| {
        view.controller.snapshot().cards[0].id.to_string()
    });
    view.update(cx, |view, cx| {
        view.command(
            Command::MoveCard {
                id,
                position: Point::new(100_000.0, 50.0),
            },
            cx,
        );
    });
    cx.run_until_parked();
    for zoom in [0.349_999, 0.35, 0.649_999, 0.65, 1.0] {
        view.update(cx, |view, cx| {
            view.command(
                Command::SetViewport(Viewport {
                    zoom,
                    ..Viewport::default()
                }),
                cx,
            );
        });
        cx.run_until_parked();
        let before = view.read_with(cx, |view, _| view.scene.borrow().shaped_rows);
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        view.read_with(cx, |view, _| {
            assert!(view.canvas.painted.is_empty());
            assert_eq!(view.scene.borrow().frame_work, scene::FrameWork::default());
            assert_eq!(view.scene.borrow().shaped_rows, before);
        });
    }
}

#[gpui::test]
fn crossing_edge_shapes_one_offscreen_endpoint_on_demand_and_culls_unseen_routes(
    cx: &mut TestAppContext,
) {
    let (fixture, _) = source_fixture(long_source(20_000), vec![]);
    let mut snapshot = fixture.snapshot().clone();
    let mut source = snapshot.cards[0].clone();
    source.position = Point::new(-10_000.0, 200.0).try_into().unwrap();
    let mut target = source.clone();
    target.id = "outside-target".into();
    target.position = Point::new(10_000.0, 200.0).try_into().unwrap();
    snapshot.cards = Arc::new(vec![source.clone(), target]);
    snapshot.connections = Arc::new(vec![
        refscape_model::Connection {
            id: "crossing".into(),
            from: source.id.clone(),
            to: "outside-target".into(),
            kind: ConnectionKind::Definition,
            source: Position::new(0, 0),
        },
        refscape_model::Connection {
            id: "below-window".into(),
            from: source.id,
            to: "outside-target".into(),
            kind: ConnectionKind::Definition,
            source: Position::new(19_999, 0),
        },
    ]);
    // Place the second route's target below the viewport as well, so its entire
    // conservative bounding box is out of view rather than crossing vertically.
    let mut below = snapshot.cards[1].clone();
    below.id = "below-target".into();
    below.position = Point::new(10_000.0, 400_000.0).try_into().unwrap();
    Arc::make_mut(&mut snapshot.cards).push(below);
    Arc::make_mut(&mut snapshot.connections)[1].to = "below-target".into();
    let (_, executor) = fixture.driver.into_parts();
    let mut fixture = FixtureDriver {
        driver: refscape_application::HeadlessDriver::new(executor),
    };
    fixture
        .dispatch(Command::OpenLoaded {
            loaded: ImportedSession { snapshot },
            destination: "session.json".into(),
            expected_root: None,
            overrides: ProjectOpenOptions::default(),
        })
        .unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::from_fixture(
            fixture,
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
    view.update(cx, |view, cx| {
        view.command(
            Command::SetViewport(Viewport {
                zoom: 0.649_999,
                ..Viewport::default()
            }),
            cx,
        );
    });
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    view.read_with(cx, |view, _| {
        assert!(view.canvas.painted.is_empty());
        assert_eq!(view.scene.borrow().frame_work, scene::FrameWork::default());
    });
    view.update(cx, |view, cx| {
        view.command(
            Command::SetViewport(Viewport {
                zoom: 0.65,
                ..Viewport::default()
            }),
            cx,
        );
    });
    cx.run_until_parked();
    let before = view.update(cx, |view, cx| {
        view.scene.borrow_mut().clear();
        cx.notify();
        view.scene.borrow().shaped_rows
    });
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    view.read_with(cx, |view, _| {
        assert!(
            view.canvas.painted.is_empty(),
            "both edge cards remain offscreen"
        );
        let cache = view.scene.borrow();
        assert_eq!(cache.frame_work.visible_rows, 0);
        assert_eq!(cache.frame_work.edge_rows, 1);
        assert_eq!(cache.frame_work.painted_edges, 1);
        assert!(
            cache.frame_work.shape_misses
                <= cache.frame_work.visible_rows + cache.frame_work.edge_rows
        );
        assert_eq!(cache.shaped_rows - before, 1);
        assert_eq!(cache.resident_rows(), 1);
    });
    let before = view.read_with(cx, |view, _| view.scene.borrow().shaped_rows);
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    view.read_with(cx, |view, _| {
        assert_eq!(view.scene.borrow().shaped_rows, before);
        assert_eq!(view.scene.borrow().frame_work.shape_misses, 0);
        assert_eq!(view.scene.borrow().frame_work.painted_edges, 1);
    });
}

#[gpui::test]
fn glyph_cache_evicts_old_rows_at_capacity_and_retains_recent_rows(cx: &mut TestAppContext) {
    let (fixture, _) = source_fixture(long_source(2_304), vec![]);
    let source = fixture.snapshot().cards[0].source.clone();
    let (_, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::from_fixture(
            fixture,
            "session.json".into(),
            vec![],
            None,
            ProjectOpenOptions::default(),
            window,
            cx,
        )
    });
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, _| {
        let mut cache = scene::SceneCache::default();
        let runs = [gpui::TextRun {
            len: 4,
            font: gpui::font("Cascadia Code"),
            color: gpui::rgb(0xffffff).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        for row in 0..2_304 {
            cache.shape(&source, row, &runs, 1.0, window);
        }
        assert_eq!(cache.shaped_rows, 2_304);
        assert_eq!(cache.resident_rows(), 2_048);
        assert_eq!(cache.evicted_rows, 256);
        cache.shape(&source, 2_303, &runs, 1.0, window);
        assert_eq!(cache.shaped_rows, 2_304, "recent rows remain resident");
        cache.shape(&source, 0, &runs, 1.0, window);
        assert_eq!(
            cache.shaped_rows, 2_305,
            "evicted rows are reshaped on demand"
        );
        assert_eq!(cache.evicted_rows, 257);
        assert_eq!(cache.resident_rows(), 2_048);
    })
    .unwrap();
}
