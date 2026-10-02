use super::*;
#[test]
fn arrange_uses_source_order_saved_root_and_keeps_unrelated_fixed() {
    for selected in [None, Some("missing".into())] {
        let mut h = connected();
        let before = h.snapshot().clone();
        let root = h.card("root").position;
        let unrelated = h.card("unrelated").position;
        h.run(Command::Arrange {
            selected: selected.clone(),
        });
        assert_eq!(h.card("root").position, root);
        assert_eq!(h.card("unrelated").position, unrelated);
        assert_eq!(
            h.card("earlier").position.x,
            h.card("root").position.x + h.card("root").width + 100.0
        );
        assert_eq!(h.card("later").position.x, h.card("earlier").position.x);
        assert!(
            h.card("earlier").position.y + h.card("earlier").display_height() + 74.0
                <= h.card("later").position.y
        );
        assert_eq!(
            h.card("grandchild").position.x,
            h.card("later").position.x + h.card("later").width + 100.0
        );
        unchanged_content(h.snapshot(), &before);
        assert_eq!(h.snapshot().viewport, before.viewport);
        let arranged = h.snapshot().clone();
        h.run(Command::Arrange { selected });
        assert_eq!(h.snapshot(), &arranged);
    }
}
#[test]
fn selecting_a_child_arranges_only_descendants() {
    let mut h = connected();
    let before = h.snapshot().clone();
    let selected = h.card("later").id.to_string();
    h.run(Command::Arrange {
        selected: Some(selected),
    });
    for original in before.cards.iter() {
        let current = h.card(&original.source.symbol.name);
        if original.source.symbol.name == "grandchild" {
            assert_ne!(current.position, original.position);
        } else {
            assert_eq!(current.position, original.position);
        }
    }
    assert_eq!(
        h.card("grandchild").position.x,
        h.card("later").position.x + h.card("later").width + 100.0
    );
    unchanged_content(h.snapshot(), &before);
}
#[test]
fn arrange_keeps_source_and_undo_restores_only_positions() {
    let mut h = connected();
    h.run(Command::Pan(Point::new(30.0, 100.0)));
    h.run(Command::Zoom {
        factor: 1.5,
        anchor: Point::new(100.0, 200.0),
    });
    h.state.lock().unwrap().fail_source = true;
    let before = h.snapshot().clone();
    h.run(Command::Arrange { selected: None });
    unchanged_content(h.snapshot(), &before);
    assert!(h.driver.controller.can_undo_layout());
    h.run(Command::Pan(Point::new(15.0, -20.0)));
    let viewport = h.snapshot().viewport;
    h.run(Command::UndoLayout);
    assert_eq!(h.snapshot().cards, before.cards);
    assert_eq!(h.snapshot().connections, before.connections);
    assert_eq!(h.snapshot().viewport, viewport);
    assert!(!h.driver.controller.can_undo_layout());
}
#[test]
fn same_position_move_intent_invalidates_layout_undo() {
    let mut h = connected();
    h.run(Command::Arrange { selected: None });
    assert!(h.driver.controller.can_undo_layout());
    let card = h.card("root");
    let id = card.id.to_string();
    let position = card.position.point();
    h.run(Command::MoveCard { id, position });
    assert!(!h.driver.controller.can_undo_layout());
}
#[test]
fn add_remove_and_fold_invalidate_undo() {
    for operation in 0..2 {
        let mut h = connected();
        h.run(Command::Arrange { selected: None });
        assert!(h.driver.controller.can_undo_layout());
        if operation == 0 {
            h.add("another", Point::new(-2000.0, 0.0));
        } else {
            let id = h.card("earlier").id.to_string();
            h.run(Command::CloseCard { id });
        }
        assert!(!h.driver.controller.can_undo_layout());
    }
}
#[test]
fn zoom_preserves_cursor_anchor_and_rejects_invalid_input() {
    let mut h = Harness::new();
    let anchor = Point::new(120.0, 70.0);
    let world = h.snapshot().viewport.screen_to_world(anchor);
    h.run(Command::Zoom {
        factor: 2.0,
        anchor,
    });
    assert_eq!(h.snapshot().viewport.world_to_screen(world), anchor);
    let before = h.snapshot().clone();
    h.run(Command::Zoom {
        factor: f32::NAN,
        anchor,
    });
    assert_eq!(h.snapshot(), &before);
    h.run(Command::Pan(Point::new(f32::INFINITY, 0.0)));
    assert_eq!(h.snapshot(), &before);
}
#[test]
fn same_point_additions_preserve_old_card_and_leave_region_padding() {
    let mut h = Harness::new();
    h.add("a", Point::default());
    let first = h.card("a").position;
    h.add("b", Point::default());
    assert_eq!(h.card("a").position, first);
    let a = refscape_canvas::layout::CardRect::from(
        &refscape_canvas::layout::LayoutCard::try_from(h.card("a")).unwrap(),
    );
    let b = refscape_canvas::layout::CardRect::from(
        &refscape_canvas::layout::LayoutCard::try_from(h.card("b")).unwrap(),
    );
    assert!(!a.overlaps(b));
    assert!(
        h.card("b").position.y >= h.card("a").display_height() + 74.0
            || h.card("b").position.x >= h.card("a").width + 74.0
    );
}
#[test]
fn moving_one_card_into_obstacle_keeps_others_fixed() {
    let mut h = Harness::new();
    let a = h.add("a", Point::default());
    h.add("b", Point::new(1200.0, 0.0));
    let b = h.card("b").position;
    h.run(Command::MoveCard {
        id: a,
        position: b.point(),
    });
    assert_eq!(h.card("b").position, b);
    assert_ne!(h.card("a").position, b);
    h.snapshot().validate().unwrap();
}
#[test]
fn closing_cards_preserves_survivor_positions_and_camera() {
    let mut h = Harness::new();
    let a = h.add("a", Point::default());
    h.add("b", Point::new(1200.0, 0.0));
    h.add("c", Point::new(0.0, 1000.0));
    h.run(Command::Pan(Point::new(30.0, 50.0)));
    let before = h.snapshot().clone();
    h.run(Command::CloseCard { id: a });
    for card in h.snapshot().cards.iter() {
        assert_eq!(
            card.position,
            before
                .cards
                .iter()
                .find(|old| old.id == card.id)
                .unwrap()
                .position
        );
    }
    assert_eq!(h.snapshot().viewport, before.viewport);
}
#[test]
fn long_cards_have_effective_source_height_and_never_overlap() {
    let mut h = Harness::new();
    let mut long = symbol("long");
    long.range.end = Position::new(500, 0);
    let code = "fn line() {}\n".repeat(500);
    h.state
        .lock()
        .unwrap()
        .sources
        .insert("long".into(), document(long.clone(), &code));
    h.run(Command::AddSymbol {
        symbol: long,
        position: Point::default(),
        toggle: false,
    });
    h.add("other", Point::default());
    assert_eq!(h.card("long").display_height(), 10076.0);
    refscape_canvas::layout::validate_layout(
        &h.snapshot()
            .cards
            .iter()
            .map(refscape_canvas::layout::LayoutCard::try_from)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap(),
        refscape_canvas::layout::LayoutRules::default(),
    )
    .unwrap();
}
#[test]
fn multiple_results_use_stable_order_and_first_representative() {
    let mut h = Harness::new();
    let root = h.add("root", Point::default());
    let mut duplicate = symbol("a");
    duplicate.id = "z-alias".into();
    h.state.lock().unwrap().targets = vec![symbol("z"), duplicate, symbol("a"), symbol("b")];
    h.run(Command::Navigate {
        card: root,
        position: Position::new(0, 4),
        kind: ConnectionKind::Definition,
        anchor: Point::new(520.0, 60.0),
        toggle: false,
    });
    let names: Vec<_> = h
        .snapshot()
        .cards
        .iter()
        .map(|card| card.source.symbol.id.as_str())
        .collect();
    assert_eq!(names, ["root", "a", "b", "z"]);
    assert_eq!(h.state.lock().unwrap().source_calls, 4);
}
#[test]
fn unfold_repairs_direct_collisions_and_collapse_never_compacts() {
    let mut h = Harness::new();
    let mut s = symbol("root");
    s.range.start = Position::new(12, 0);
    s.range.end = Position::new(12, 14);
    s.selection_range.start.line = 12;
    s.selection_range.end.line = 12;
    let mut doc = document(s.clone(), "fn target() {}");
    doc.context = vec![SourceContext {
        start_line: 0,
        code: "impl Project {".into(),
    }];
    doc.folded = vec![SourceContext {
        start_line: 1,
        code: format!("{}\n{}", "x".repeat(120), "    call();\n".repeat(10)),
    }];
    h.state.lock().unwrap().sources.insert("root".into(), doc);
    h.run(Command::AddSymbol {
        symbol: s,
        position: Point::new(100.0, 80.0),
        toggle: false,
    });
    let root = h.card("root").id.to_string();
    let child = h.expand(&root, 12, "child");
    let before = h.card("root").position;
    let child_before = h.card("child").position;
    h.state.lock().unwrap().fail_source = true;
    h.run(Command::ToggleFold {
        card: root.clone(),
        index: 0,
        expand: true,
    });
    assert_eq!(h.card("root").position, before);
    assert_eq!(
        h.card("root").source.display_row(Position::new(12, 4)),
        Some(12)
    );
    assert_ne!(h.card("child").position, child_before);
    let expanded = h.card("root").source.clone();
    let child_after = h.card("child").position;
    h.run(Command::ToggleFold {
        card: root.clone(),
        index: 0,
        expand: false,
    });
    assert_eq!(h.card("root").width, 520.0);
    assert_eq!(h.card("root").display_height(), 136.0);
    assert_eq!(h.card("child").position, child_after);
    assert_eq!(h.snapshot().connections[0].to, child);
    h.run(Command::ToggleFold {
        card: root,
        index: 0,
        expand: true,
    });
    assert_eq!(h.card("root").source, expanded);
}
#[test]
fn fit_preserves_existing_margin_and_zoom_policy() {
    let mut h = Harness::new();
    h.add("a", Point::new(100.0, 200.0));
    h.run(Command::Fit {
        width: 600.0,
        height: 400.0,
    });
    assert_eq!(h.snapshot().viewport.zoom, 1.0);
    assert_eq!(h.snapshot().viewport.offset, Point::new(-60.0, -160.0));
}
#[test]
fn metadata_groups_by_deepest_official_package_root() {
    let mut h = Harness::new();
    let mut snapshot = h.snapshot().clone();
    snapshot.project_root = h.root.clone();
    let mut a = symbol("a");
    a.path = h.root.join("nested/a.rs");
    let mut b = symbol("b");
    b.path = h.root.join("b.rs");
    h.state.lock().unwrap().crates = vec![
        ProjectCrate {
            id: "outer".into(),
            name: "outer".into(),
            root: h.root.clone(),
        },
        ProjectCrate {
            id: "inner".into(),
            name: "inner".into(),
            root: h.root.join("nested"),
        },
    ];
    h.import(snapshot);
    h.run(Command::AddSymbol {
        symbol: a,
        position: Point::default(),
        toggle: false,
    });
    h.run(Command::AddSymbol {
        symbol: b,
        position: Point::new(1000.0, 0.0),
        toggle: false,
    });
    assert!(
        h.snapshot()
            .regions
            .iter()
            .any(|r| r.label == "inner" && r.card_ids.contains(&h.card("a").id))
    );
    assert!(
        h.snapshot()
            .regions
            .iter()
            .any(|r| r.label == "outer" && r.card_ids.contains(&h.card("b").id))
    );
}
