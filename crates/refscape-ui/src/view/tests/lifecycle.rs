use super::*;

#[gpui::test]
fn a_drop_during_a_read_request_waits_for_the_backend_and_commits_once(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
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
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.search(cx);
        let card = &view.session.cards[0];
        view.canvas.drag = Some(Drag::Card(
            card.id.clone(),
            point(px(0.0), px(0.0)),
            card.position,
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
        assert_eq!(view.session.cards[0].position, Point::new(100.0, 50.0));
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.requests.busy);
        assert_eq!(view.session.cards[0].position, Point::new(200.0, 100.0));
        assert!(view.layout.pending_drop.is_none());
        assert_eq!(
            view.session.cards,
            view.explorer.lock().unwrap().session().cards
        );
    });
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
            ProjectOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.search(cx);
        let anchor = Point::new(400.0, 200.0);
        let before = view.session.viewport.screen_to_world(anchor);
        view.zoom(1.5, anchor, cx);
        assert_eq!(before, view.session.viewport.screen_to_world(anchor));
        view.canvas.drag = Some(Drag::Pan(point(px(0.0), px(0.0))));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(35.0), px(60.0)),
                ..Default::default()
            },
            cx,
        );
        view.canvas.drag = Some(Drag::Card(
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
        assert!(view.requests.error);
        assert_eq!(view.requests.status, "simulated analyzer failure");
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
            ProjectOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    view.update_in(cx, |view, window, cx| {
        assert_eq!(view.project.themes.len(), 3);
        view.requests.busy = true;
        assert!(!view.close(window, cx));
        assert!(!view.requests.closing);
        view.requests.busy = false;
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
