use super::*;
#[gpui::test]
fn source_hover_is_debounced_and_uses_absolute_utf16_at_each_zoom(cx: &mut TestAppContext) {
    let (explorer, requests) = fixture();
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
    let handle = cx.window_handle();
    for (zoom, offset) in [
        (1.0, Point::default()),
        (0.75, Point::new(30.0, 45.0)),
        (1.5, Point::new(-40.0, 10.0)),
    ] {
        view.update(cx, |view, cx| {
            view.clear_hover(cx);
            view.session.viewport.zoom = zoom;
            view.session.viewport.offset = offset;
            cx.notify();
        });
        requests.lock().unwrap().clear();
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let (word, other_glyph, header, empty) = view.read_with(cx, |view, _| {
            let card = &view.canvas.painted[0];
            (
                point(
                    card.origin.x + card.rows[0].code.x_for_index("日本😀".len()) + px(1.0),
                    card.origin.y + px(5.0 * zoom),
                ),
                point(
                    card.origin.x + card.rows[0].code.x_for_index("日本😀ca".len()) + px(1.0),
                    card.origin.y + px(5.0 * zoom),
                ),
                point(card.bounds.left() + px(20.0), card.bounds.top() + px(10.0)),
                point(
                    view.canvas.bounds.right() - px(10.0),
                    view.canvas.bounds.bottom() - px(10.0),
                ),
            )
        });
        cx.simulate_mouse_move(word, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
        assert!(requests.lock().unwrap().is_empty());
        cx.simulate_mouse_move(header, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        assert!(
            requests.lock().unwrap().is_empty(),
            "leaving before the delay cancels the request"
        );
        cx.simulate_mouse_move(word, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(400));
        cx.run_until_parked();
        assert_eq!(*requests.lock().unwrap(), vec![Position::new(12, 9)]);
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.hover.text.as_deref(),
                Some("fn call() -> u32\n\nCalls the helper.")
            );
            assert!(!view.requests.busy);
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        cx.simulate_mouse_move(other_glyph, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        assert_eq!(
            requests.lock().unwrap().len(),
            1,
            "moving inside the same word reuses its hover"
        );
        let panel = cx.debug_bounds("code-hover").expect("rendered hover panel");
        view.read_with(cx, |view, _| {
            assert!(panel.left() >= view.canvas.bounds.left());
            assert!(panel.right() <= view.canvas.bounds.right());
            assert!(panel.top() >= view.canvas.bounds.top());
            assert!(panel.bottom() <= view.canvas.bounds.bottom());
        });
        let inside_panel = point(panel.left() + px(20.0), panel.top() + px(20.0));
        let gap = point(word.x, panel.top() - px(3.0));
        cx.simulate_mouse_move(gap, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(
                view.hover.text.is_some(),
                "crossing the popup gap keeps it open"
            );
        });
        cx.simulate_mouse_move(inside_panel, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(
                view.hover.text.is_some(),
                "the popup remains readable while hovered"
            )
        });
        cx.simulate_click(inside_panel, Modifiers::default());
        assert_eq!(
            requests.lock().unwrap().len(),
            1,
            "clicks inside the popup must not open definitions"
        );
        cx.simulate_mouse_move(empty, None, Modifiers::default());
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        view.read_with(cx, |view, _| assert!(view.hover.text.is_none()));
    }
}

#[gpui::test]
fn long_hover_documentation_scrolls_without_moving_the_canvas(cx: &mut TestAppContext) {
    let (explorer, requests) = fixture();
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
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let word = view.read_with(cx, |view, _| {
        let card = &view.canvas.painted[0];
        point(
            card.origin.x + card.rows[0].code.x_for_index("日本😀".len()) + px(1.0),
            card.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_move(word, None, Modifiers::default());
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        // A real pointer path can pass over a word on the next source row.
        view.session.cards[0].source.code.push_str("\n日本😀other");
        view.session.cards[0].source.symbol.range.end = Position::new(13, 9);
        view.hover.text = Some(
            (0..80)
                .map(|line| format!("Documentation line {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let panel = cx.debug_bounds("code-hover").unwrap();
    let inside = point(panel.left() + px(20.0), panel.top() + px(20.0));
    cx.simulate_mouse_move(
        point(word.x, panel.top() - px(3.0)),
        None,
        Modifiers::default(),
    );
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.hover.text.is_some()));
    cx.simulate_mouse_move(inside, None, Modifiers::default());
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    let viewport = view.read_with(cx, |view, _| view.session.viewport);
    cx.simulate_event(ScrollWheelEvent {
        position: inside,
        delta: ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
        ..Default::default()
    });
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    view.read_with(cx, |view, _| {
        assert!(view.hover.text.is_some());
        assert!(
            view.hover.scroll.offset().y < px(0.0),
            "documentation actually scrolls"
        );
        assert_eq!(view.session.viewport, viewport);
        assert_eq!(requests.lock().unwrap().len(), 1);
    });
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |view, _| {
        assert!(view.hover.text.is_none());
        assert_eq!(view.hover.scroll.offset().y, px(0.0));
    });
}

#[gpui::test]
fn hover_cancels_when_zooming_and_never_requests_hidden_source(cx: &mut TestAppContext) {
    let (explorer, requests) = fixture();
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
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let word = view.read_with(cx, |view, _| {
        let card = &view.canvas.painted[0];
        point(
            card.origin.x + card.rows[0].code.x_for_index("日本😀".len()) + px(1.0),
            card.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_move(word, None, Modifiers::default());
    view.update(cx, |view, cx| view.zoom(0.5, Point::default(), cx));
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    cx.simulate_mouse_move(word, None, Modifiers::default());
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    assert!(requests.lock().unwrap().is_empty());
    view.read_with(cx, |view, _| assert!(view.hover.target.is_none()));
}
