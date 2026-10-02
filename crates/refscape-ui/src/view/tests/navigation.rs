use super::*;

#[gpui::test]
fn reusing_a_card_highlights_the_target_and_preserves_every_position(cx: &mut TestAppContext) {
    let target = Symbol::file(
        "target.rs".into(),
        SourceRange {
            start: Position::new(12, 5),
            end: Position::new(12, 13),
        },
    );
    let (mut explorer, _) = fixture_with_targets(vec![target.clone()]);
    let target_id = explorer
        .add_symbol(target, Point::new(800.0, 50.0))
        .unwrap();
    let before = explorer.snapshot().cards.clone();
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
        point(
            card.origin.x + card.rows[0].code.x_for_index("日本😀".len()) + px(1.0),
            card.origin.y + px(5.0),
        )
    });
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.requests.error, "{}", view.requests.status);
        assert_eq!(view.controller.snapshot().cards, before);
        assert_eq!(view.canvas.selected.as_deref(), Some(target_id.as_str()));
        assert_eq!(view.controller.snapshot().connections.len(), 1);
    });
}

#[gpui::test]
fn clicking_a_linked_word_toggles_cards_even_at_different_glyphs(cx: &mut TestAppContext) {
    let mut target = Symbol::file(
        "target.rs".into(),
        SourceRange {
            start: Position::new(12, 5),
            end: Position::new(12, 13),
        },
    );
    target.id = "target".into();
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
    let handle = cx.window_handle();
    for (glyph, count) in [("日本😀", 2), ("日本😀ca", 1), ("日本😀c", 2)] {
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
            assert_eq!(view.controller.snapshot().connections.len(), count - 1);
        });
    }
    // Hiding an existing link is local and should not ask the analyzer again.
    assert_eq!(requests.lock().unwrap().len(), 2);
    let picker_symbol = view.read_with(cx, |view, _| {
        view.controller.snapshot().cards[1].source.symbol.clone()
    });
    view.update(cx, |view, cx| {
        view.canvas.selected = Some(view.controller.snapshot().cards[1].id.to_string());
        view.toggle_symbol(picker_symbol.clone(), cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.controller.snapshot().cards.len(), 1);
        assert!(view.controller.snapshot().connections.is_empty());
        assert!(view.canvas.selected.is_none());
    });
    view.update(cx, |view, cx| view.toggle_symbol(picker_symbol, cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.controller.snapshot().cards.len(), 2)
    });
}
