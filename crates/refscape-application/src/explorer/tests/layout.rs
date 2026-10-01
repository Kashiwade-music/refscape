use super::*;
use refscape_canvas::layout::CARD_GAP;
use refscape_model::{CODE_CARD_HEADER, CODE_LINE_HEIGHT};

fn positions(cards: &[CodeCard]) -> Vec<(String, Point)> {
    cards
        .iter()
        .map(|card| (card.id.clone(), card.position))
        .collect()
}

fn assert_nonoverlapping(cards: &[CodeCard]) {
    for (index, card) in cards.iter().enumerate() {
        for other in &cards[index + 1..] {
            assert!(
                !CardRect::from(card).overlaps(CardRect::from(other)),
                "{} overlaps {}",
                card.id,
                other.id
            );
        }
    }
}

#[test]
fn unfolding_context_repairs_collisions_and_preserves_navigation_and_transactions() {
    let mut explorer = explorer();
    let mut parent = symbol("root");
    parent.range.start = Position::new(12, 0);
    parent.range.end = Position::new(12, 14);
    parent.selection_range.start.line = 12;
    parent.selection_range.end.line = 12;
    let root = explorer
        .add_symbol(parent, Point::new(100.0, 80.0))
        .unwrap();
    explorer.session.cards[0]
        .source
        .context
        .push(refscape_model::SourceContext {
            start_line: 0,
            code: "impl Project {".into(),
        });
    explorer.session.cards[0]
        .source
        .folded
        .push(refscape_model::SourceContext {
            start_line: 1,
            code: format!("{}\n{}", "x".repeat(120), "    call();\n".repeat(10)),
        });
    let child = explorer
        .expand_definition(&root, Position::new(12, 4))
        .unwrap()
        .remove(0);
    let child_before = explorer
        .session
        .cards
        .iter()
        .find(|card| card.id == child)
        .unwrap()
        .position;
    let root_before = explorer.session.cards[0].position;
    assert!(
        explorer
            .expand_definition(&root, Position::new(1, 4))
            .is_err()
    );
    let before = explorer.session.clone();
    assert!(explorer.expand_context(&root, 4).is_err());
    assert_eq!(explorer.session, before);
    explorer.language.fail = true;
    explorer.expand_context(&root, 0).unwrap(); // New snapshots do not reread externally modified source.
    let card = explorer
        .session
        .cards
        .iter()
        .find(|card| card.id == root)
        .unwrap();
    assert!(card.source.folded.is_empty());
    assert_eq!(card.source.display_row(Position::new(12, 4)), Some(12));
    assert_eq!(card.position, root_before);
    let child_after = explorer
        .session
        .cards
        .iter()
        .find(|card| card.id == child)
        .unwrap();
    assert_ne!(child_after.position, child_before);
    assert!(!CardRect::from(card).overlaps(CardRect::from(child_after)));
    explorer.language.fail = false;
    explorer.language.target = symbol("context_target");
    let targets = explorer
        .expand_definition(&root, Position::new(0, 5))
        .unwrap();
    assert_eq!(targets.len(), 1);
    assert!(explorer.hover(&root, Position::new(1, 4)).is_ok());
    explorer
        .expand_definition(&root, Position::new(1, 4))
        .unwrap();
    let expanded_source = explorer
        .session
        .cards
        .iter()
        .find(|card| card.id == root)
        .unwrap()
        .source
        .clone();
    let child_before_collapse = explorer
        .session
        .cards
        .iter()
        .find(|card| card.id == child)
        .unwrap()
        .position;
    let before = explorer.session.clone();
    assert!(explorer.collapse_context(&root, 4).is_err());
    assert_eq!(explorer.session, before);
    explorer.language.fail = true;
    explorer.collapse_context(&root, 0).unwrap();
    let card = explorer
        .session
        .cards
        .iter()
        .find(|card| card.id == root)
        .unwrap();
    assert_eq!(card.source.context[0].code, "impl Project {");
    assert!(card.source.expanded.is_empty());
    assert_eq!(card.source.folded[0].code.lines().count(), 11);
    assert_eq!(card.width, 520.0);
    assert_eq!(card.display_height(), 136.0);
    assert_eq!(
        source_anchor_y(card, Position::new(1, 4)),
        card.position.y + CODE_CARD_HEADER + 8.0 + CODE_LINE_HEIGHT
    );
    assert_eq!(card.position, root_before);
    assert_eq!(
        explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == child)
            .unwrap()
            .position,
        child_before_collapse
    );
    assert!(explorer.hover(&root, Position::new(1, 4)).is_err());
    explorer.expand_context(&root, 0).unwrap();
    assert_eq!(
        explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == root)
            .unwrap()
            .source,
        expanded_source
    );
    explorer.session.validate().unwrap();
}

#[test]
fn legacy_context_gaps_load_through_the_backend_and_failure_preserves_the_card() {
    let mut explorer = explorer();
    let mut parent = symbol("root");
    parent.range.start.line = 3;
    parent.range.end.line = 3;
    parent.selection_range.start.line = 3;
    parent.selection_range.end.line = 3;
    let root = explorer.add_symbol(parent, Point::default()).unwrap();
    explorer.session.cards[0]
        .source
        .context
        .push(refscape_model::SourceContext {
            start_line: 0,
            code: "impl Project {".into(),
        });
    let before = explorer.session.clone();
    explorer.language.fail = true;
    assert!(explorer.expand_context(&root, 0).is_err());
    assert_eq!(explorer.session, before);
    explorer.language.fail = false;
    explorer.language.code = "impl Project {\n    fn first() {}\n\n    fn target() {}\n}".into();
    explorer.expand_context(&root, 0).unwrap();
    assert!(
        explorer.session.cards[0].source.context[0]
            .code
            .contains("    fn first() {}")
    );
    assert_eq!(
        explorer.session.cards[0]
            .source
            .display_row(Position::new(3, 4)),
        Some(3)
    );
}

#[test]
fn expanding_an_earlier_line_preserves_siblings_and_their_descendants() {
    for references in [false, true] {
        let mut explorer = explorer();
        explorer.language.code = std::iter::repeat_n("    call();", 30)
            .collect::<Vec<_>>()
            .join("\n");
        let mut parent = symbol("parent");
        parent.range.end = Position::new(30, 0);
        let root = explorer
            .add_symbol(parent, Point::new(100.0, 80.0))
            .unwrap();
        explorer.language.code = "fn target() {}".into();
        let later = if references {
            explorer.expand_references(&root, Position::new(19, 4))
        } else {
            explorer.expand_definition(&root, Position::new(19, 4))
        }
        .unwrap()
        .remove(0);
        explorer.language.target = symbol("grandchild");
        let grandchild = explorer
            .expand_definition(&later, Position::new(0, 4))
            .unwrap()
            .remove(0);
        let before = explorer.session.cards.clone();
        explorer.language.target = symbol("earlier");
        let earlier = if references {
            explorer.expand_references(&root, Position::new(18, 4))
        } else {
            explorer.expand_definition(&root, Position::new(18, 4))
        }
        .unwrap()
        .remove(0);
        let card = |id: &str| {
            explorer
                .session
                .cards
                .iter()
                .find(|card| card.id == id)
                .unwrap()
        };
        assert_eq!(card(&root).position, Point::new(100.0, 80.0));
        for original in &before {
            assert_eq!(card(&original.id).position, original.position);
        }
        assert!(card(&earlier).position.x >= card(&root).position.x + card(&root).width + 100.0);
        assert!(!CardRect::from(card(&earlier)).overlaps(CardRect::from(card(&later))));
        explorer.remove_card(&earlier).unwrap();
        for original in &before {
            let current = explorer
                .session
                .cards
                .iter()
                .find(|card| card.id == original.id)
                .unwrap();
            assert_eq!(current.position, original.position);
        }
        assert!(
            explorer
                .session
                .cards
                .iter()
                .any(|card| card.id == grandchild)
        );
        explorer.session.validate().unwrap();
    }
}

#[test]
fn expansion_uses_absolute_source_rows_and_deletion_preserves_the_anchor() {
    for references in [false, true] {
        let mut explorer = explorer();
        explorer.language.code = std::iter::repeat_n("    call();", 100)
            .collect::<Vec<_>>()
            .join("\n");
        let mut source = symbol("long_parent");
        source.range = SourceRange {
            start: Position::new(20, 5),
            end: Position::new(120, 0),
        };
        source.selection_range = SourceRange {
            start: Position::new(20, 5),
            end: Position::new(20, 9),
        };
        let root = explorer
            .add_symbol(source, Point::new(100.0, 80.0))
            .unwrap();
        let unrelated = explorer
            .add_symbol(symbol("unrelated"), Point::new(0.0, 3000.0))
            .unwrap();
        explorer.language.code = "fn target() {}".into();
        let position = Position::new(70, 4);
        let child = if references {
            explorer.expand_references(&root, position)
        } else {
            explorer.expand_definition(&root, position)
        }
        .unwrap()
        .remove(0);
        let expected_offset = CODE_CARD_HEADER + 8.0 + 50.0 * CODE_LINE_HEIGHT;
        let assert_anchor = |explorer: &Explorer<Language, Repository>| {
            let parent = explorer
                .session
                .cards
                .iter()
                .find(|card| card.id == root)
                .unwrap();
            let target = explorer
                .session
                .cards
                .iter()
                .find(|card| card.id == child)
                .unwrap();
            assert_eq!(target.position.y, parent.position.y + expected_offset);
            assert_eq!(target.position.x, parent.position.x + parent.width + 100.0);
            assert!(!CardRect::from(parent).overlaps(CardRect::from(target)));
        };
        assert_anchor(&explorer);
        explorer.remove_card(&unrelated).unwrap();
        assert_anchor(&explorer);
        explorer.session.validate().unwrap();
    }
}

#[test]
fn same_point_additions_keep_existing_cards_and_leave_region_padding() {
    let mut explorer = explorer();
    explorer
        .add_symbol(symbol("first"), Point::default())
        .unwrap();
    explorer
        .add_symbol(symbol("second"), Point::default())
        .unwrap();
    let cards = &explorer.session.cards;
    // The file frame extends 22 below each card and 36 above the next.
    assert!(cards[0].position.y + cards[0].display_height() + 22.0 < cards[1].position.y - 36.0);
    assert_eq!(cards[0].position, Point::default());
}

#[test]
fn closing_distant_cards_preserves_positions_when_a_lower_card_is_wider() {
    let mut explorer = explorer();
    let root = explorer
        .add_symbol(symbol("root"), Point::default())
        .unwrap();
    let child = explorer
        .expand_definition(&root, Position::new(0, 4))
        .unwrap()
        .remove(0);
    explorer.language.code = "x".repeat(150);
    let wide = explorer
        .add_symbol(symbol("wide"), Point::new(0.0, 1000.0))
        .unwrap();
    let unrelated = explorer
        .add_symbol(symbol("unrelated"), Point::new(2000.0, 2000.0))
        .unwrap();
    let viewport = explorer.session.viewport;
    let positions = explorer
        .session
        .cards
        .iter()
        .filter(|card| card.id != unrelated)
        .map(|card| (card.id.clone(), card.position))
        .collect::<Vec<_>>();
    explorer.remove_card(&unrelated).unwrap();
    let root = explorer
        .session
        .cards
        .iter()
        .find(|card| card.id == root)
        .unwrap();
    let child = explorer
        .session
        .cards
        .iter()
        .find(|card| card.id == child)
        .unwrap();
    let wide = explorer
        .session
        .cards
        .iter()
        .find(|card| card.id == wide)
        .unwrap();
    assert_eq!(root.position.x, wide.position.x);
    assert_eq!(child.position.x, root.position.x + root.width + 100.0);
    assert_eq!(child.position.y, source_anchor_y(root, Position::new(0, 4)));
    for (id, position) in positions {
        assert_eq!(
            explorer
                .session
                .cards
                .iter()
                .find(|card| card.id == id)
                .unwrap()
                .position,
            position
        );
    }
    assert_eq!(explorer.session.viewport, viewport);
    for (index, card) in explorer.session.cards.iter().enumerate() {
        for other in &explorer.session.cards[index + 1..] {
            assert!(!CardRect::from(card).overlaps(CardRect::from(other)));
        }
    }
}

#[test]
fn hiding_middle_and_edge_cards_preserves_all_surviving_positions_and_viewport() {
    let mut explorer = explorer();
    explorer.language.code = std::iter::repeat_n("fn source() {}", 20)
        .collect::<Vec<_>>()
        .join("\n");
    let height = CodeCard::source_height(&SourceDocument {
        expanded: Vec::new(),
        folded: Vec::new(),
        context: Vec::new(),
        code_start: None,
        symbol: symbol("root"),
        code: explorer.language.code.clone(),
        tokens: vec![],
    });
    let root = explorer
        .add_symbol(symbol("root"), Point::new(100.0, 80.0))
        .unwrap();
    let middle = explorer
        .add_symbol(
            symbol("middle"),
            Point::new(100.0, 80.0 + height + CARD_GAP),
        )
        .unwrap();
    let bottom = explorer
        .add_symbol(
            symbol("bottom"),
            Point::new(100.0, 80.0 + 2.0 * (height + CARD_GAP)),
        )
        .unwrap();
    let column = explorer
        .add_symbol(symbol("column"), Point::new(720.0, 80.0))
        .unwrap();
    let far = explorer
        .add_symbol(symbol("far"), Point::new(1340.0, 80.0))
        .unwrap();
    explorer.pan(Point::new(-150.0, 65.0)).unwrap();
    explorer.zoom(0.75, Point::new(200.0, 180.0)).unwrap();
    let viewport = explorer.session.viewport;
    explorer
        .toggle_symbol(symbol("middle"), Point::default())
        .unwrap();
    assert!(!explorer.session.cards.iter().any(|card| card.id == middle));
    assert_eq!(
        explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == bottom)
            .unwrap()
            .position,
        Point::new(100.0, 80.0 + 2.0 * (height + CARD_GAP))
    );
    explorer.remove_card(&column).unwrap();
    assert_eq!(
        explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == far)
            .unwrap()
            .position,
        Point::new(1340.0, 80.0)
    );
    assert_eq!(
        explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == root)
            .unwrap()
            .position,
        Point::new(100.0, 80.0)
    );
    assert_eq!(explorer.session.viewport, viewport);
    for (index, card) in explorer.session.cards.iter().enumerate() {
        for other in &explorer.session.cards[index + 1..] {
            assert!(!CardRect::from(card).overlaps(CardRect::from(other)));
        }
    }
    let before = explorer.session.clone();
    assert!(explorer.remove_card("missing").is_err());
    assert_eq!(explorer.session, before);
    explorer.session.validate().unwrap();
}

#[test]
fn zoom_preserves_cursor_anchor_and_rejects_invalid_input() {
    let mut explorer = explorer();
    explorer.pan(Point::new(15.0, -22.0)).unwrap();
    let cursor = Point::new(400.0, 240.0);
    let world = explorer.session.viewport.screen_to_world(cursor);
    explorer.zoom(1.5, cursor).unwrap();
    assert_eq!(explorer.session.viewport.world_to_screen(world), cursor);
    let before = explorer.session.clone();
    assert!(explorer.zoom(f32::NAN, cursor).is_err());
    assert!(explorer.pan(Point::new(f32::INFINITY, 0.0)).is_err());
    assert_eq!(explorer.session, before);
}

#[test]
fn failed_backend_expansion_and_sync_leave_canvas_unchanged() {
    let mut explorer = explorer();
    let origin = explorer
        .add_symbol(symbol("origin"), Point::default())
        .unwrap();
    let before = explorer.session.clone();
    explorer.language.fail = true;
    assert!(
        explorer
            .expand_definition(&origin, Position::new(0, 4))
            .is_err()
    );
    assert!(
        explorer
            .sync_canvas(
                Viewport::default(),
                vec![
                    (origin, Point::new(5.0, 5.0)),
                    ("missing".into(), Point::default())
                ]
            )
            .is_err()
    );
    assert_eq!(explorer.session, before);
}

#[test]
fn new_and_expanded_long_cards_do_not_overlap_or_clip_source() {
    let mut explorer = explorer();
    let line = "x".repeat(180);
    explorer.language.code = std::iter::repeat_n(line, 70).collect::<Vec<_>>().join("\n");
    explorer.language.additional.push(symbol("second_target"));
    let origin = explorer
        .add_symbol(symbol("origin"), Point::new(100.0, 80.0))
        .unwrap();
    let picker_card = explorer
        .add_symbol(symbol("picked"), Point::new(100.0, 80.0))
        .unwrap();
    let origin_rect = CardRect::from(&explorer.session.cards[0]);
    assert_eq!(origin_rect.width, 1520.0);
    assert_eq!(origin_rect.height, 1476.0);
    assert_ne!(
        explorer.session.cards[0].position,
        explorer.session.cards[1].position
    );
    // Occupy the natural first target location before expanding two tall sources.
    explorer
        .move_card(
            &picker_card,
            Point::new(
                origin_rect.position.x + origin_rect.width + 100.0,
                origin_rect.position.y,
            ),
        )
        .unwrap();
    let targets = explorer
        .expand_definition(&origin, Position::new(0, 4))
        .unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(explorer.session.cards.len(), 4);
    for (index, card) in explorer.session.cards.iter().enumerate() {
        for other in &explorer.session.cards[index + 1..] {
            assert!(
                !CardRect::from(card).overlaps(CardRect::from(other)),
                "{} overlaps {}",
                card.id,
                other.id
            );
        }
    }
    explorer.session.validate().unwrap();
}

#[test]
fn restore_repairs_only_overlapping_cards_and_sync_rejects_invalid_layouts() {
    let mut original = explorer();
    original.language.code = std::iter::repeat_n("fn tall() {}", 40)
        .collect::<Vec<_>>()
        .join("\n");
    for name in ["first", "second", "third", "clear"] {
        original.add_symbol(symbol(name), Point::default()).unwrap();
    }
    // An old snapshot used a fixed height and stacked cards using that height.
    for (index, card) in original.session.cards.iter_mut().enumerate() {
        card.height = 128.0;
        card.position = Point::new(0.0, index as f32 * 160.0);
    }
    let clear = Point::new(800.0, 20.0);
    original.session.cards[3].position = clear;
    let saved = original.session.clone();
    struct Saved(Session);
    impl SessionRepository for Saved {
        fn save(&self, _: &Path, _: &Session) -> Result<()> {
            Ok(())
        }
        fn load(&self, _: &Path) -> Result<Session> {
            Ok(self.0.clone())
        }
    }
    let mut restored = Explorer::new(original.language, Saved(saved));
    restored
        .load_session(Path::new("old-session.json"))
        .unwrap();
    let cards = &restored.session.cards;
    assert_eq!(cards[0].position, Point::default());
    assert_eq!(cards[0].height, 128.0);
    assert_eq!(cards[0].display_height(), 876.0);
    assert_eq!(cards[3].position, clear);
    for (index, card) in cards.iter().enumerate() {
        for other in &cards[index + 1..] {
            assert!(!CardRect::from(card).overlaps(CardRect::from(other)));
        }
    }
    let before = restored.session.clone();
    restored.repository.0 = before.clone();
    restored
        .load_session(Path::new("old-session.json"))
        .unwrap();
    assert_eq!(restored.session, before);

    // UI movement must use the same complete rectangles before expanding/saving.
    let positions = restored.session.cards[..3]
        .iter()
        .map(|card| (card.id.clone(), Point::default()))
        .collect();
    assert!(
        restored
            .sync_canvas(Viewport::default(), positions)
            .is_err()
    );
    assert_eq!(restored.session, before);
    for (index, card) in restored.session.cards.iter().enumerate() {
        for other in &restored.session.cards[index + 1..] {
            assert!(!CardRect::from(card).overlaps(CardRect::from(other)));
        }
    }
    assert_eq!(restored.session.cards[3].position, clear);
}

#[test]
fn multiple_results_are_placed_in_stable_order_after_backend_deduplication() {
    let mut snapshots = Vec::new();
    for reverse in [false, true] {
        let mut explorer = explorer();
        let root = explorer
            .add_symbol(symbol("root"), Point::new(100.0, 80.0))
            .unwrap();
        explorer.language.target = symbol(if reverse { "zeta" } else { "alpha" });
        explorer.language.additional = vec![
            symbol("middle"),
            symbol(if reverse { "alpha" } else { "zeta" }),
        ];
        let targets = explorer
            .expand_definition(&root, Position::new(0, 4))
            .unwrap();
        assert_eq!(targets.len(), 3);
        let mut snapshot = explorer
            .session
            .cards
            .iter()
            .map(|card| (card.source.symbol.id.clone(), card.position))
            .collect::<Vec<_>>();
        snapshot.sort_by(|left, right| left.0.cmp(&right.0));
        snapshots.push(snapshot);
        assert_nonoverlapping(&explorer.session.cards);
    }
    assert_eq!(snapshots[0], snapshots[1]);
}

#[test]
fn equivalent_sources_choose_the_same_representative_for_reversed_backend_results() {
    let mut snapshots = Vec::new();
    for reverse in [false, true] {
        let mut explorer = explorer();
        let root = explorer
            .add_symbol(symbol("root"), Point::default())
            .unwrap();
        let canonical = symbol("alpha");
        let mut alias = canonical.clone();
        alias.id = "zeta".into();
        alias.name = "alias".into();
        explorer.language.target = if reverse {
            alias.clone()
        } else {
            canonical.clone()
        };
        explorer.language.additional = vec![if reverse { canonical } else { alias }];
        let targets = explorer
            .expand_definition(&root, Position::new(0, 4))
            .unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(explorer.session.cards[1].source.symbol.id, "alpha");
        snapshots.push(explorer.session.cards.clone());
    }
    assert_eq!(snapshots[0], snapshots[1]);
}

#[test]
fn self_references_cycles_and_multiple_parents_reuse_cards_in_place() {
    let mut explorer = explorer();
    let root = explorer
        .add_symbol(symbol("root"), Point::default())
        .unwrap();
    explorer.language.target = symbol("child");
    let child = explorer
        .expand_definition(&root, Position::new(0, 4))
        .unwrap()
        .remove(0);
    let other = explorer
        .add_symbol(symbol("other"), Point::new(0.0, 1000.0))
        .unwrap();
    let before = positions(&explorer.session.cards);
    explorer.language.target = symbol("root");
    assert_eq!(
        explorer
            .expand_definition(&root, Position::new(0, 4))
            .unwrap(),
        vec![root.clone()]
    );
    assert_eq!(positions(&explorer.session.cards), before);
    assert_eq!(
        explorer
            .expand_references(&child, Position::new(0, 4))
            .unwrap(),
        vec![root]
    );
    assert_eq!(positions(&explorer.session.cards), before);
    explorer.language.target = symbol("child");
    assert_eq!(
        explorer
            .expand_definition(&other, Position::new(0, 4))
            .unwrap(),
        vec![child]
    );
    assert_eq!(positions(&explorer.session.cards), before);
    assert_eq!(explorer.session.cards.len(), 3);
    explorer.session.validate().unwrap();
}

#[test]
fn resizing_preserves_target_and_only_moves_direct_collisions_then_shrinks_in_place() {
    for code in [
        std::iter::repeat_n("fn tall() {}", 40)
            .collect::<Vec<_>>()
            .join("\n"),
        "x".repeat(180),
        std::iter::repeat_n("x".repeat(180), 40)
            .collect::<Vec<_>>()
            .join("\n"),
    ] {
        let mut explorer = explorer();
        explorer
            .add_symbol(symbol("root"), Point::default())
            .unwrap();
        explorer
            .add_symbol(symbol("right"), Point::new(620.0, 0.0))
            .unwrap();
        explorer
            .add_symbol(symbol("below"), Point::new(0.0, 202.0))
            .unwrap();
        explorer
            .add_symbol(symbol("far"), Point::new(3000.0, 3000.0))
            .unwrap();
        let before = explorer.session.cards.clone();
        let original_source = before[0].source.clone();
        let mut enlarged = original_source.clone();
        enlarged.code = code;
        let (width, height) = source_dimensions(&enlarged);
        let target = CardRect {
            position: before[0].position,
            width,
            height,
        };
        let colliders = before[1..]
            .iter()
            .filter(|card| target.overlaps(CardRect::from(*card)))
            .map(|card| card.id.clone())
            .collect::<Vec<_>>();
        assert!(!colliders.is_empty());
        explorer.replace_card_source(0, enlarged).unwrap();
        assert_eq!(explorer.session.cards[0].position, before[0].position);
        for original in &before[1..] {
            let current = explorer
                .session
                .cards
                .iter()
                .find(|card| card.id == original.id)
                .unwrap();
            if colliders.contains(&original.id) {
                assert_ne!(current.position, original.position);
            } else {
                assert_eq!(current.position, original.position);
            }
        }
        assert_nonoverlapping(&explorer.session.cards);
        let enlarged_positions = positions(&explorer.session.cards);
        explorer.replace_card_source(0, original_source).unwrap();
        assert_eq!(positions(&explorer.session.cards), enlarged_positions);
        assert_eq!(explorer.session.cards[0].width, before[0].width);
        assert_eq!(explorer.session.cards[0].height, before[0].height);
        assert_nonoverlapping(&explorer.session.cards);
    }
}

#[test]
fn failed_resize_keeps_source_dimensions_connections_positions_and_regions() {
    let mut explorer = explorer();
    let root = explorer
        .add_symbol(symbol("root"), Point::default())
        .unwrap();
    explorer
        .expand_definition(&root, Position::new(0, 4))
        .unwrap();
    let mut invalid_source = explorer.session.cards[0].source.clone();
    invalid_source.symbol.range.end = Position::new(0, 0);
    invalid_source.symbol.range.start = Position::new(1, 0);
    let before = explorer.session.clone();
    assert!(explorer.replace_card_source(0, invalid_source).is_err());
    assert_eq!(explorer.session, before);
    explorer.session.cards[1].position = Point::new(f32::MAX, f32::MAX);
    let mut changed_source = explorer.session.cards[0].source.clone();
    changed_source.code = "x".repeat(180);
    let before = explorer.session.clone();
    assert!(explorer.replace_card_source(0, changed_source).is_err());
    assert_eq!(explorer.session, before);
}

#[test]
fn moving_one_card_into_an_obstacle_keeps_every_other_card_fixed() {
    let mut explorer = explorer();
    let moving = explorer
        .add_symbol(symbol("moving"), Point::default())
        .unwrap();
    let obstacle = explorer
        .add_symbol(symbol("obstacle"), Point::new(620.0, 0.0))
        .unwrap();
    let before = explorer.session.cards[1].position;
    explorer.move_card(&moving, before).unwrap();
    assert_eq!(
        explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == obstacle)
            .unwrap()
            .position,
        before
    );
    assert_ne!(explorer.session.cards[0].position, before);
    assert_nonoverlapping(&explorer.session.cards);
    let snapshot = explorer.session.clone();
    assert!(
        explorer
            .move_card(&moving, Point::new(f32::MAX, f32::MAX))
            .is_err()
    );
    assert_eq!(explorer.session, snapshot);
}

#[test]
fn sync_save_hover_theme_and_viewport_changes_keep_valid_card_positions() {
    let mut explorer = explorer();
    let root = explorer
        .add_symbol(symbol("root"), Point::new(-200.0, -100.0))
        .unwrap();
    explorer
        .add_symbol(symbol("other"), Point::new(620.0, 500.0))
        .unwrap();
    let before = positions(&explorer.session.cards);
    explorer
        .sync_canvas(explorer.session.viewport, before.clone())
        .unwrap();
    explorer.pan(Point::new(40.0, -20.0)).unwrap();
    explorer.zoom(1.5, Point::new(300.0, 100.0)).unwrap();
    explorer.set_theme(Theme::light()).unwrap();
    explorer.hover(&root, Position::new(0, 4)).unwrap();
    explorer.save_session(Path::new("session.json")).unwrap();
    assert_eq!(positions(&explorer.session.cards), before);
}

#[test]
fn unopened_empty_canvas_synchronizes_its_camera_but_cannot_be_saved() {
    let original = explorer();
    let mut unopened = Explorer::new(original.language, Repository);
    assert!(unopened.session.project_root.as_os_str().is_empty());
    let viewport = Viewport {
        offset: Point::new(100.0, -80.0),
        zoom: 1.5,
    };
    unopened.sync_canvas(viewport, Vec::new()).unwrap();
    assert_eq!(unopened.session.viewport, viewport);
    assert!(unopened.session.cards.is_empty());
    assert!(unopened.session.connections.is_empty());
    let before = unopened.session.clone();
    assert!(unopened.save_session(Path::new("unopened.json")).is_err());
    assert_eq!(unopened.session, before);
}

mod prepared;
mod rearrange;
