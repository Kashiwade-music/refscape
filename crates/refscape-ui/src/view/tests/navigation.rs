use super::*;
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
    for (glyph, count) in [("日本😀", 2), ("日本😀ca", 1), ("日本😀c", 2)] {
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let click = view.read_with(cx, |view, _| {
            let source = &view.canvas.painted[0];
            point(
                source.origin.x + source.lines[0].x_for_index(glyph.len()) + px(1.0),
                source.origin.y + px(5.0),
            )
        });
        cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(!view.requests.error, "{}", view.requests.status);
            assert_eq!(view.session.cards.len(), count);
            assert_eq!(view.session.connections.len(), count - 1);
        });
    }
    // Hiding an existing link is local and should not ask the analyzer again.
    assert_eq!(requests.lock().unwrap().len(), 2);
    let picker_symbol = view.read_with(cx, |view, _| view.session.cards[1].source.symbol.clone());
    view.update(cx, |view, cx| {
        view.canvas.selected = Some(view.session.cards[1].id.clone());
        view.toggle_symbol(picker_symbol.clone(), cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards.len(), 1);
        assert!(view.session.connections.is_empty());
        assert!(view.canvas.selected.is_none());
    });
    view.update(cx, |view, cx| view.toggle_symbol(picker_symbol, cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.session.cards.len(), 2));
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
            ProjectOptions::default(),
            window,
            cx,
        )
    });
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let click = view.read_with(cx, |view, _| {
        let card = &view.canvas.painted[0];
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
