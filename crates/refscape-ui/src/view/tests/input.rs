//! Native text-input callback traces use UTF-16 ranges, including composition.
use super::*;
use gpui::EntityInputHandler;
#[gpui::test]
fn ime_composition_replacement_and_selection_keep_utf16_contract(cx: &mut TestAppContext) {
    let (fixture, _) = fixture();
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
    view.update_in(cx, |view, window, cx| {
        view.search.focused = true;
        EntityInputHandler::replace_and_mark_text_in_range(
            view,
            None,
            "日本😀",
            Some(2..4),
            window,
            cx,
        );
        assert_eq!(view.search.query, "日本😀");
        assert_eq!(view.search.marked, Some(0..10));
        assert_eq!(view.search.selection, 6..10);
        assert_eq!(
            EntityInputHandler::selected_text_range(view, false, window, cx)
                .unwrap()
                .range,
            2..4
        );
        assert_eq!(
            EntityInputHandler::text_length_utf16(view, window, cx),
            Some(4)
        );
        EntityInputHandler::replace_and_mark_text_in_range(
            view,
            None,
            "日本語",
            Some(3..3),
            window,
            cx,
        );
        assert_eq!(view.search.marked, Some(0..9));
        assert_eq!(view.search.selection, 9..9);
        EntityInputHandler::unmark_text(view, window, cx);
        assert!(view.search.marked.is_none());
        EntityInputHandler::replace_text_in_range(view, Some(2..3), "😀", window, cx);
        assert_eq!(view.search.query, "日本😀");
        let mut adjusted = None;
        assert_eq!(
            EntityInputHandler::text_for_range(view, 1..4, &mut adjusted, window, cx),
            Some("本😀".into())
        );
        assert_eq!(adjusted, Some(1..4));
        EntityInputHandler::set_selected_text_range(view, 0..4, window, cx);
        assert_eq!(view.search.selection, 0..10);
        assert!(EntityInputHandler::accepts_text_input(view, window, cx));
    });
}
