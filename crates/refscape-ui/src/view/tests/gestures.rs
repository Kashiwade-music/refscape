//! GPUI event-routing traces for capture, release and display scale changes.
use super::*;
#[gpui::test]
fn left_and_middle_canvas_gestures_end_on_outside_release_cancel_and_focus_loss(
    cx: &mut TestAppContext,
) {
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
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let (inside, outside) = view.read_with(cx, |view, _| {
        (
            point(
                view.canvas.bounds.right() - px(20.0),
                view.canvas.bounds.bottom() - px(20.0),
            ),
            point(
                view.canvas.bounds.left() - px(40.0),
                view.canvas.bounds.top() + px(70.0),
            ),
        )
    });
    for button in [MouseButton::Left, MouseButton::Middle] {
        cx.simulate_mouse_down(inside, button, Modifiers::default());
        view.read_with(cx, |view, _| {
            assert!(matches!(view.canvas.drag,Some(Drag::Pan(start,_)) if start==button))
        });
        cx.simulate_mouse_move(
            point(inside.x - px(10.0), inside.y - px(20.0)),
            Some(button),
            Modifiers::default(),
        );
        cx.simulate_mouse_up(outside, button, Modifiers::default());
        cx.run_until_parked();
        let camera = view.read_with(cx, |view, _| {
            assert!(view.canvas.drag.is_none());
            view.controller.snapshot().viewport
        });
        cx.simulate_mouse_move(inside, None, Modifiers::default());
        view.read_with(cx, |view, _| {
            assert_eq!(view.controller.snapshot().viewport, camera)
        });
    }
    cx.simulate_mouse_down(inside, MouseButton::Middle, Modifiers::default());
    cx.simulate_mouse_up(inside, MouseButton::Left, Modifiers::default());
    view.read_with(cx, |view, _| {
        assert!(matches!(
            view.canvas.drag,
            Some(Drag::Pan(MouseButton::Middle, _))
        ))
    });
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |view, _| assert!(view.canvas.drag.is_none()));

    cx.update_window(handle, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    cx.simulate_mouse_down(inside, MouseButton::Middle, Modifiers::default());
    let (second, _) = super::fixtures::fixture();
    let (_, second_cx) = cx.add_window_view(|window, cx| {
        ExplorerView::from_fixture(
            second,
            "other-session.json".into(),
            vec![],
            None,
            ProjectOpenOptions::default(),
            window,
            cx,
        )
    });
    second_cx.update(|window, _| window.activate_window());
    second_cx.run_until_parked();
    view.read_with(second_cx, |view, _| {
        assert!(view.canvas.drag.is_none());
        assert!(view.canvas.drag_preview.is_none());
        assert!(!view.controller.dragging());
    });
}
#[gpui::test]
fn display_scale_invalidates_device_glyph_cache_and_keeps_source_hit_mapping(
    cx: &mut TestAppContext,
) {
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
    let handle = cx.window_handle();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let count = view.read_with(cx, |view, _| view.scene.borrow().shaped_rows);
    let old_scale = cx
        .update_window(handle, |_, window, _| window.scale_factor())
        .unwrap();
    cx.simulate_scale_factor_change(old_scale + 1.0);
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    assert_eq!(
        cx.update_window(handle, |_, window, _| window.scale_factor())
            .unwrap(),
        old_scale + 1.0
    );
    view.read_with(cx, |view, _| {
        assert!(view.scene.borrow().shaped_rows > count);
        let row = view.canvas.painted[0].row(0).unwrap();
        assert_eq!(
            row.source_position(row.code.x_for_index("日本😀".len()) + px(1.0)),
            Some(Position::new(12, 9))
        );
    });
}
