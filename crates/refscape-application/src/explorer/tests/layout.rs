use super::*;
use refscape_model::{CODE_CARD_HEADER, CODE_LINE_HEIGHT};

#[test]
fn unfolding_context_reanchors_children_and_preserves_navigation_and_transactions() {
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
    let before_y = explorer
        .session
        .cards
        .iter()
        .find(|card| card.id == child)
        .unwrap()
        .position
        .y;
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
    assert_eq!(
        explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == child)
            .unwrap()
            .position
            .y,
        before_y + 10.0 * CODE_LINE_HEIGHT
    );
    assert_eq!(
        explorer
            .session
            .cards
            .iter()
            .find(|child_card| child_card.id == child)
            .unwrap()
            .position
            .x,
        card.position.x + card.width + CARD_COLUMN_GAP
    );
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
    assert!(
        explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == child)
            .unwrap()
            .position
            .y
            >= source_anchor_y(card, Position::new(12, 4))
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
fn expanding_an_earlier_line_reorders_siblings_and_their_descendants() {
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
        assert_eq!(
            card(&earlier).position.y,
            source_anchor_y(card(&root), Position::new(18, 4))
        );
        assert!(
            card(&earlier).position.y + card(&earlier).display_height() + CARD_GAP
                <= card(&later).position.y
        );
        assert_eq!(
            card(&grandchild).position.y,
            source_anchor_y(card(&later), Position::new(0, 4))
        );
        assert_eq!(card(&earlier).position.x, card(&later).position.x);
        explorer.remove_card(&earlier).unwrap();
        let later = explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == later)
            .unwrap();
        let root = explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == root)
            .unwrap();
        assert_eq!(
            later.position.y,
            source_anchor_y(root, Position::new(19, 4))
        );
        explorer.session.validate().unwrap();
    }
}

#[test]
fn expansion_and_compaction_anchor_children_to_the_absolute_source_row() {
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
            assert_eq!(
                target.position.x,
                parent.position.x + parent.width + CARD_COLUMN_GAP
            );
            assert!(!CardRect::from(parent).overlaps(CardRect::from(target)));
        };
        assert_anchor(&explorer);
        explorer.remove_card(&unrelated).unwrap();
        assert_anchor(&explorer);
        explorer.session.validate().unwrap();
    }
}

#[test]
fn stacked_cards_leave_room_for_file_region_headers_and_padding() {
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
    arrange_cards(&mut explorer.session.cards).unwrap();
    let cards = &explorer.session.cards;
    assert!(cards[0].position.y + cards[0].display_height() + 22.0 < cards[1].position.y - 36.0);
}

#[test]
fn closing_cards_preserves_columns_when_a_lower_card_is_wider() {
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
    assert!(child.position.x >= wide.position.x + wide.width + CARD_COLUMN_GAP);
    assert_eq!(child.position.y, source_anchor_y(root, Position::new(0, 4)));
    assert_eq!(explorer.session.viewport, viewport);
    for (index, card) in explorer.session.cards.iter().enumerate() {
        for other in &explorer.session.cards[index + 1..] {
            assert!(!CardRect::from(card).overlaps(CardRect::from(other)));
        }
    }
}

#[test]
fn hiding_cards_packs_rows_and_columns_without_changing_the_viewport() {
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
        Point::new(100.0, 80.0 + height + CARD_GAP)
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
        Point::new(720.0, 80.0)
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
fn restore_and_canvas_sync_clear_full_source_heights_and_preserve_clear_cards() {
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
    assert_eq!(cards[0].height, 876.0);
    assert_eq!(cards[1].position.y, 876.0 + CARD_GAP);
    assert_eq!(cards[2].position.y, 2.0 * (876.0 + CARD_GAP));
    assert_eq!(cards[3].position, clear);
    let before = cards.clone();
    arrange_cards(&mut restored.session.cards).unwrap();
    assert_eq!(restored.session.cards, before);

    // UI movement must use the same complete rectangles before expanding/saving.
    let positions = restored.session.cards[..3]
        .iter()
        .map(|card| (card.id.clone(), Point::default()))
        .collect();
    restored
        .sync_canvas(Viewport::default(), positions)
        .unwrap();
    for (index, card) in restored.session.cards.iter().enumerate() {
        for other in &restored.session.cards[index + 1..] {
            assert!(!CardRect::from(card).overlaps(CardRect::from(other)));
        }
    }
    assert_eq!(restored.session.cards[3].position, clear);
}
