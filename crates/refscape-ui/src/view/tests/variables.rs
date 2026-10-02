use super::*;
#[gpui::test]
fn variable_click_highlights_identity_and_toggles_type_at_any_glyph(cx: &mut TestAppContext) {
    let (explorer, requests, type_requests) = variable_fixture(true);
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
    for (glyph, count) in [("日本😀ca", 2), ("日本😀c", 1), ("日本😀", 2)] {
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let click = view.read_with(cx, |view, _| {
            let source = &view.canvas.painted[0];
            point(
                source.origin.x + source.rows[0].code.x_for_index(glyph.len()) + px(1.0),
                source.origin.y + px(5.0),
            )
        });
        cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(!view.requests.error, "{}", view.requests.status);
            assert_eq!(view.controller.snapshot().cards.len(), count);
            let origin = &view.controller.snapshot().cards[0];
            assert_eq!(
                variable_highlight_spans(origin, 0, view.canvas.inspection.as_ref()),
                vec!["日本😀".len().."日本😀call".len()]
            );
            assert_eq!(
                variable_highlight_spans(origin, 1, view.canvas.inspection.as_ref()),
                vec![4..8]
            );
            if count == 2 {
                assert_eq!(
                    view.controller.snapshot().connections[0].kind,
                    ConnectionKind::TypeDefinition
                );
                assert_eq!(
                    view.controller.snapshot().connections[0].source,
                    Position::new(12, 9)
                );
                assert_eq!(
                    card_title(
                        view.controller.snapshot(),
                        &view.controller.snapshot().cards[1]
                    ),
                    "call → Config"
                );
                assert!(
                    variable_highlight_spans(
                        &view.controller.snapshot().cards[1],
                        0,
                        view.canvas.inspection.as_ref()
                    )
                    .is_empty()
                );
            }
        });
    }
    assert_eq!(
        *type_requests.lock().unwrap(),
        vec![Position::new(12, 9); 2]
    );
    // Only hover asks the ordinary request recorder; no binding-definition navigation.
    assert_eq!(*requests.lock().unwrap(), vec![Position::new(12, 9); 3]);
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |view, _| assert!(view.canvas.inspection.is_none()));
}

#[gpui::test]
fn primitive_variable_keeps_highlights_and_alt_click_opens_binding(cx: &mut TestAppContext) {
    let (explorer, requests, type_requests) = variable_fixture(false);
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
    view.read_with(cx, |view, _| {
        assert_eq!(view.controller.snapshot().cards.len(), 1);
        assert!(view.canvas.inspection.is_some());
        assert_eq!(
            view.canvas
                .inspection
                .as_ref()
                .unwrap()
                .description
                .as_deref(),
            Some("let call: u32")
        );
        assert!(!view.requests.error);
    });
    assert_eq!(type_requests.lock().unwrap().len(), 1);
    cx.simulate_mouse_down(
        click,
        MouseButton::Left,
        Modifiers {
            alt: true,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.canvas.inspection.is_none());
        assert_eq!(view.controller.snapshot().cards.len(), 2);
        assert_eq!(
            view.controller.snapshot().connections[0].kind,
            ConnectionKind::Definition
        );
    });
    assert_eq!(*requests.lock().unwrap(), vec![Position::new(12, 9); 2]);
}

#[gpui::test]
fn clearing_selection_during_analysis_does_not_restore_stale_highlights(cx: &mut TestAppContext) {
    let (explorer, _, _) = variable_fixture(false);
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
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.requests.busy);
        assert!(!view.requests.error);
        assert!(view.canvas.inspection.is_none());
    });
}
