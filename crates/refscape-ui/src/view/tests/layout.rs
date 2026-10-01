use super::*;

pub(super) fn connected_explorer() -> Explorer<Language, Repository> {
    let range = SourceRange {
        start: Position::new(12, 5),
        end: Position::new(12, 13),
    };
    let (mut explorer, _) = fixture_with_targets(vec![
        Symbol::file("far.rs".into(), range),
        Symbol::file("lower.rs".into(), range),
    ]);
    let root = explorer.session().cards[0].id.clone();
    let children = explorer
        .expand_definition(&root, Position::new(12, 9))
        .unwrap();
    explorer
        .move_card(&children[0], Point::new(5000.0, 50.0))
        .unwrap();
    explorer
        .move_card(&children[1], Point::new(2500.0, 1500.0))
        .unwrap();
    explorer
        .add_symbol(
            Symbol::file("unrelated.rs".into(), range),
            Point::new(-4000.0, 50.0),
        )
        .unwrap();
    explorer
}

#[gpui::test]
fn explicit_arrange_places_the_connected_tree_and_undo_restores_positions(cx: &mut TestAppContext) {
    let explorer = connected_explorer();
    let before = explorer.session().cards.clone();
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
    view.update(cx, |view, cx| view.arrange_layout(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_ne!(view.session.cards, before);
        assert_eq!(view.session.cards[0], before[0]);
        assert_eq!(view.session.cards[3], before[3]);
        let children = &view.session.cards[1..3];
        assert_eq!(
            children[0].position.x,
            before[0].position.x + before[0].width + 100.0
        );
        assert_eq!(children[0].position.x, children[1].position.x);
        assert!(children[0].position.y < children[1].position.y);
        assert!(view.layout.can_undo);
        refscape_canvas::layout::validate_layout(&view.session.cards, Default::default()).unwrap();
    });
    view.update(cx, |view, cx| view.undo_layout(cx));
    cx.run_until_parked();
    cx.background_executor
        .advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards, before);
        assert!(!view.layout.can_undo);
    });
}

#[gpui::test]
fn ordinary_addition_and_deletion_leave_survivors_fixed_after_idle(cx: &mut TestAppContext) {
    let explorer = connected_explorer();
    let before = explorer.session().cards.clone();
    let added = Symbol::file("extra.rs".into(), before[0].source.symbol.range);
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
    view.update(cx, |view, cx| view.toggle_symbol(added.clone(), cx));
    cx.run_until_parked();
    cx.background_executor
        .advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards.len(), before.len() + 1);
        assert_eq!(&view.session.cards[..before.len()], before.as_slice());
    });
    view.update(cx, |view, cx| view.toggle_symbol(added, cx));
    cx.run_until_parked();
    cx.background_executor
        .advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards, before);
        assert!(!view.layout.can_undo);
    });
}

#[gpui::test]
fn camera_input_and_idle_leave_card_positions_unchanged(cx: &mut TestAppContext) {
    let explorer = connected_explorer();
    let before = explorer.session().cards.clone();
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
    view.update(cx, |view, cx| view.zoom(1.25, Point::new(200.0, 100.0), cx));
    cx.background_executor
        .advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards, before);
        assert_eq!(view.session.viewport.zoom, 1.25);
    });
}

#[gpui::test]
fn arrange_waits_for_analysis_shutdown_and_active_pan(cx: &mut TestAppContext) {
    let explorer = connected_explorer();
    let before = explorer.session().cards.clone();
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
        view.requests.busy = true;
        view.arrange_layout(cx);
        view.requests.busy = false;
        view.requests.closing = true;
        view.arrange_layout(cx);
        view.requests.closing = false;
        view.canvas.drag = Some(Drag::Pan(point(px(0.0), px(0.0))));
        view.arrange_layout(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.session.cards, before));
    view.update(cx, |view, cx| {
        view.finish_drag(cx);
        view.arrange_layout(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_ne!(view.session.cards, before);
        assert!(view.layout.can_undo);
    });
}

#[gpui::test]
fn an_inflight_arrange_replans_the_latest_selected_leaf_and_viewport(cx: &mut TestAppContext) {
    let explorer = connected_explorer();
    let before = explorer.session().cards.clone();
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
        view.arrange_layout(cx);
        view.canvas.selected = Some(before[1].id.clone());
        view.zoom(1.25, Point::default(), cx);
    });
    let viewport = view.read_with(cx, |view, _| view.session.viewport);
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards, before);
        assert!(!view.requests.busy);
        assert_eq!(view.session.viewport, viewport);
        assert!(!view.layout.can_undo);
    });
}
#[gpui::test]
fn sidebar_selection_during_arrange_replans_only_the_latest_root_tree(cx: &mut TestAppContext) {
    let mut explorer = connected_explorer();
    let latest_root = explorer.session().cards[1].id.clone();
    // The existing second sibling also becomes a descendant of this root.
    explorer
        .expand_definition(&latest_root, Position::new(12, 9))
        .unwrap();
    let before = explorer.session().cards.clone();
    let symbol = before[1].source.symbol.clone();
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
    let old_commit = view.read_with(cx, |view, _| {
        view.explorer
            .lock()
            .unwrap()
            .plan_layout(Some(&before[0].id))
            .unwrap()
    });
    assert!(
        old_commit.changed,
        "the old root must have a distinct placement candidate"
    );
    view.update(cx, |view, cx| {
        view.canvas.selected = Some(before[0].id.clone());
        view.requests.busy = true;
        view.layout.planning = true;
        let epoch = view.layout.session_epoch;
        let revision = view.layout.revision;
        view.toggle_symbol(symbol, cx);
        assert_eq!(view.canvas.selected.as_ref(), Some(&latest_root));
        assert_eq!(
            view.session.cards, before,
            "busy sidebar selection must not remove a card"
        );
        assert_ne!(view.layout.revision, revision);
        view.complete_layout(Ok(old_commit), epoch, revision, cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards[0], before[0]);
        assert_eq!(view.session.cards[1], before[1]);
        assert_eq!(view.session.cards[3], before[3]);
        assert_ne!(view.session.cards[2].position, before[2].position);
        assert!(view.session.cards[2].position.x >= before[1].position.x + before[1].width + 100.0);
        assert_eq!(view.canvas.selected.as_ref(), Some(&latest_root));
        assert!(!view.requests.busy);
        assert!(!view.requests.error, "{}", view.requests.status);
        assert!(view.layout.can_undo);
        refscape_canvas::layout::validate_layout(&view.session.cards, Default::default()).unwrap();
    });
}

#[gpui::test]
fn dropping_a_card_invalidates_the_previous_layout_undo(cx: &mut TestAppContext) {
    let explorer = connected_explorer();
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
    view.update(cx, |view, cx| view.arrange_layout(cx));
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        assert!(view.layout.can_undo);
        let card = &view.session.cards[1];
        view.canvas.drag = Some(Drag::Card(
            card.id.clone(),
            point(px(0.0), px(0.0)),
            card.position,
        ));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(1000.0), px(0.0)),
                ..Default::default()
            },
            cx,
        );
        view.finish_drag(cx);
    });
    cx.run_until_parked();
    let dropped = view.read_with(cx, |view, _| {
        assert!(!view.layout.can_undo);
        view.session.cards.clone()
    });
    view.update(cx, |view, cx| view.undo_layout(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert_eq!(view.session.cards, dropped));
}

#[gpui::test]
fn arrange_waits_for_an_inflight_drag_and_then_fixes_the_latest_selected_card(
    cx: &mut TestAppContext,
) {
    let explorer = connected_explorer();
    let before = explorer.session().cards.clone();
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
        view.arrange_layout(cx);
        let card = &view.session.cards[1];
        view.canvas.selected = Some(card.id.clone());
        view.canvas.drag = Some(Drag::Card(
            card.id.clone(),
            point(px(0.0), px(0.0)),
            card.position,
        ));
        view.mouse_move(
            &MouseMoveEvent {
                position: point(px(1000.0), px(0.0)),
                ..Default::default()
            },
            cx,
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards, before);
        assert!(view.canvas.drag_preview.is_some());
    });
    view.update(cx, |view, cx| view.finish_drag(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards[1].position, Point::new(6000.0, 50.0));
        assert!(!view.requests.busy);
        assert!(!view.layout.can_undo);
        assert!(view.canvas.drag_preview.is_none());
        refscape_canvas::layout::validate_layout(&view.session.cards, Default::default()).unwrap();
    });
}

#[gpui::test]
fn measured_symbol_anchor_is_stable_across_camera_and_glyphs(cx: &mut TestAppContext) {
    let range = SourceRange {
        start: Position::new(12, 5),
        end: Position::new(13, 0),
    };
    let source = SourceDocument {
        symbol: Symbol::file("sample.rs".into(), range),
        code: "\t日本😀call".into(),
        tokens: vec![refscape_model::SemanticToken {
            line: 12,
            start: 10,
            length: 4,
            kind: "function".into(),
            modifiers: vec![],
        }],
        code_start: None,
        context: vec![refscape_model::SourceContext {
            start_line: 9,
            code: "\timpl 日本😀Sample {".into(),
        }],
        folded: vec![refscape_model::SourceContext {
            start_line: 10,
            code: "    hidden();\n\n".into(),
        }],
        expanded: vec![],
    };
    let (explorer, _) = source_fixture(source, vec![]);
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
    let mut expected = None;
    for (zoom, offset) in [
        (1.0, Point::default()),
        (0.75, Point::new(40.0, 60.0)),
        (1.5, Point::new(-20.0, 10.0)),
    ] {
        view.update(cx, |view, cx| {
            view.session.viewport.zoom = zoom;
            view.session.viewport.offset = offset;
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        view.read_with(cx, |view, _| {
            let card = &view.session.cards[0];
            let painted = &view.canvas.painted[0];
            let anchor =
                shaping::symbol_anchor_offset(card, painted, Position::new(12, 10)).unwrap();
            assert_eq!(anchor.y, HEADER + 8.0 + 2.0 * LINE);
            assert_eq!(
                anchor.x,
                card.source.code_gutter_width()
                    + f32::from(painted.rows[2].world_code.x_for_index("\t日本😀call".len()))
            );
            for glyph in [10, 11, 12, 13] {
                assert_eq!(
                    shaping::symbol_anchor_offset(card, painted, Position::new(12, glyph)),
                    Some(anchor)
                );
            }
            assert!(shaping::symbol_anchor_offset(card, painted, Position::new(10, 4)).is_none());
            if let Some(expected) = expected {
                assert_eq!(anchor, expected);
            } else {
                expected = Some(anchor);
            }
        });
    }
}
