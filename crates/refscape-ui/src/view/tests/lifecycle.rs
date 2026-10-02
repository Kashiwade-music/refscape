use super::*;

#[gpui::test]
fn a_drop_during_a_read_request_waits_for_the_backend_and_commits_once(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
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
    let before_geometry = view.read_with(cx, |view, _| view.controller.basis().geometry.0);
    view.update(cx, |view, cx| {
        view.search(cx);
        let card = &view.controller.snapshot().cards[0];
        view.canvas.drag = Some(Drag::Card(
            card.id.to_string(),
            point(px(0.0), px(0.0)),
            card.position.point(),
        ));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(100.0), px(50.0)),
                ..Default::default()
            },
            cx,
        );
        view.finish_drag(cx);
        assert!(view.requests.busy);
        assert_eq!(
            view.controller.snapshot().cards[0].position,
            Point::new(100.0, 50.0)
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.requests.busy);
        assert_eq!(
            view.controller.snapshot().cards[0].position,
            Point::new(200.0, 100.0)
        );
        assert!(view.canvas.drag_preview.is_none());
        assert!(!view.controller.dragging());
        assert_eq!(view.controller.basis().geometry.0, before_geometry + 1);
    });
}

#[gpui::test]
fn asynchronous_failure_keeps_canvas_movement_and_pointer_zoom_anchor(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
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

    view.update(cx, |view, cx| {
        view.search(cx);
        let anchor = Point::new(400.0, 200.0);
        let before = view.controller.snapshot().viewport.screen_to_world(anchor);
        view.zoom(1.5, anchor, cx);
        assert_eq!(
            before,
            view.controller.snapshot().viewport.screen_to_world(anchor)
        );
        view.canvas.drag = Some(Drag::Pan(MouseButton::Left, point(px(0.0), px(0.0))));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(35.0), px(60.0)),
                ..Default::default()
            },
            cx,
        );
        view.canvas.drag = Some(Drag::Card(
            view.controller.snapshot().cards[0].id.to_string(),
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
        (
            view.controller.snapshot().viewport,
            view.controller.snapshot().cards[0].position,
        )
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.requests.error);
        assert_eq!(view.requests.status, "simulated analyzer failure");
        assert_eq!(
            (
                view.controller.snapshot().viewport,
                view.controller.snapshot().cards[0].position
            ),
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
        ExplorerView::from_fixture(
            explorer,
            PathBuf::from("session.json"),
            vec![Theme::dark(), custom.clone()],
            None,
            ProjectOpenOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    view.update_in(cx, |view, window, cx| {
        assert_eq!(view.project.themes.len(), 3);
        view.search(cx);
        assert!(!view.close(window, cx));
        assert!(!view.requests.closing);
    });
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.cycle_theme(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.controller.snapshot().theme, Theme::dark())
    });
    view.update(cx, |view, cx| view.cycle_theme(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.controller.snapshot().theme, Theme::light())
    });
    view.update(cx, |view, cx| view.cycle_theme(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.controller.snapshot().theme, custom)
    });
}
