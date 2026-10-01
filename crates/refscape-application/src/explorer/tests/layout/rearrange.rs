use super::*;
use refscape_canvas::layout::CARD_RIGHT_GAP;

fn connected_canvas() -> Explorer<Language, Repository> {
    let mut explorer = explorer();
    explorer.language.code = std::iter::repeat_n("    call();", 30)
        .collect::<Vec<_>>()
        .join("\n");
    let mut parent = symbol("root");
    parent.range.end = Position::new(30, 0);
    let root = explorer
        .add_symbol(parent, Point::new(1000.0, 500.0))
        .unwrap();
    explorer.language.code = "fn target() {}".into();
    explorer.language.target = symbol("later");
    let later = explorer
        .expand_definition(&root, Position::new(19, 4))
        .unwrap()
        .remove(0);
    explorer.language.target = symbol("grandchild");
    let grandchild = explorer
        .expand_definition(&later, Position::new(0, 4))
        .unwrap()
        .remove(0);
    explorer.language.target = symbol("earlier");
    let earlier = explorer
        .expand_definition(&root, Position::new(4, 4))
        .unwrap()
        .remove(0);
    explorer
        .add_symbol(symbol("unrelated"), Point::new(-1000.0, -1000.0))
        .unwrap();
    explorer
        .move_card(&later, Point::new(5000.0, 3000.0))
        .unwrap();
    explorer
        .move_card(&grandchild, Point::new(8000.0, 6000.0))
        .unwrap();
    explorer
        .move_card(&earlier, Point::new(4000.0, 4500.0))
        .unwrap();
    explorer
}

fn card<'a>(explorer: &'a Explorer<Language, Repository>, name: &str) -> &'a CodeCard {
    explorer
        .session
        .cards
        .iter()
        .find(|card| card.source.symbol.name == name)
        .unwrap()
}

fn assert_content_preserved(current: &Session, before: &Session) {
    assert_eq!(current.cards.len(), before.cards.len());
    for (card, original) in current.cards.iter().zip(&before.cards) {
        assert_eq!(card.id, original.id);
        assert_eq!(card.source, original.source);
        assert_eq!((card.width, card.height), (original.width, original.height));
    }
    assert_eq!(current.connections, before.connections);
    assert_eq!(current.viewport, before.viewport);
}

#[test]
fn arrange_uses_source_order_and_saved_first_card_as_root_and_keeps_unrelated_cards_fixed() {
    for selected in [None, Some("missing")] {
        let mut explorer = connected_canvas();
        let before = explorer.session.clone();
        let root = card(&explorer, "root").position;
        let unrelated = card(&explorer, "unrelated").position;
        assert!(explorer.arrange_layout(selected).unwrap());
        assert_eq!(card(&explorer, "root").position, root);
        assert_eq!(card(&explorer, "unrelated").position, unrelated);
        let root = card(&explorer, "root");
        let earlier = card(&explorer, "earlier");
        let later = card(&explorer, "later");
        let grandchild = card(&explorer, "grandchild");
        assert_eq!(
            earlier.position.x,
            root.position.x + root.width + CARD_RIGHT_GAP
        );
        assert_eq!(later.position.x, earlier.position.x);
        assert!(earlier.position.y + earlier.display_height() + CARD_GAP <= later.position.y);
        assert_eq!(
            grandchild.position.x,
            later.position.x + later.width + CARD_RIGHT_GAP
        );
        assert_content_preserved(&explorer.session, &before);
        assert_nonoverlapping(&explorer.session.cards);
        let arranged = explorer.session.clone();
        assert!(!explorer.arrange_layout(selected).unwrap());
        assert_eq!(explorer.session, arranged);
    }
}

#[test]
fn selecting_a_child_arranges_only_its_descendants_and_keeps_ancestors_and_siblings_fixed() {
    let mut explorer = connected_canvas();
    let selected = card(&explorer, "later").id.clone();
    let before = explorer.session.clone();
    assert!(explorer.arrange_layout(Some(&selected)).unwrap());
    for original in &before.cards {
        let current = explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == original.id)
            .unwrap();
        if original.source.symbol.name == "grandchild" {
            assert_ne!(current.position, original.position);
        } else {
            assert_eq!(current.position, original.position);
        }
    }
    let selected = card(&explorer, "later");
    let grandchild = card(&explorer, "grandchild");
    assert_eq!(
        grandchild.position.x,
        selected.position.x + selected.width + CARD_RIGHT_GAP
    );
    assert_content_preserved(&explorer.session, &before);
    assert_nonoverlapping(&explorer.session.cards);
}

#[test]
fn explicit_arrange_keeps_source_snapshots_and_camera_and_undo_restores_only_positions() {
    let mut explorer = connected_canvas();
    explorer.pan(Point::new(30.0, 100.0)).unwrap();
    explorer.zoom(1.5, Point::new(100.0, 200.0)).unwrap();
    explorer.language.fail = true;
    let before = explorer.session.clone();
    assert!(explorer.arrange_layout(None).unwrap());
    assert_content_preserved(&explorer.session, &before);
    assert_nonoverlapping(&explorer.session.cards);
    assert!(explorer.can_undo_layout());
    explorer.pan(Point::new(15.0, -20.0)).unwrap();
    let current_viewport = explorer.session.viewport;
    assert!(explorer.undo_layout().unwrap());
    assert_eq!(explorer.session.cards, before.cards);
    assert_eq!(explorer.session.connections, before.connections);
    assert_eq!(explorer.session.viewport, current_viewport);
    assert!(!explorer.can_undo_layout());
}

#[test]
fn tree_arrangement_is_applied_even_when_bounding_area_increases() {
    let area = |cards: &[CodeCard]| {
        let left = cards
            .iter()
            .map(|card| f64::from(card.position.x))
            .reduce(f64::min)
            .unwrap();
        let top = cards
            .iter()
            .map(|card| f64::from(card.position.y))
            .reduce(f64::min)
            .unwrap();
        let right = cards
            .iter()
            .map(|card| f64::from(card.position.x) + f64::from(card.width))
            .reduce(f64::max)
            .unwrap();
        let bottom = cards
            .iter()
            .map(|card| f64::from(card.position.y) + f64::from(card.display_height()))
            .reduce(f64::max)
            .unwrap();
        (right - left) * (bottom - top)
    };
    let mut explorer = explorer();
    let root = explorer
        .add_symbol(symbol("root"), Point::default())
        .unwrap();
    explorer.language.target = symbol("child");
    let child = explorer
        .expand_definition(&root, Position::new(0, 4))
        .unwrap()
        .remove(0);
    explorer.language.target = symbol("grandchild");
    let grandchild = explorer
        .expand_definition(&child, Position::new(0, 4))
        .unwrap()
        .remove(0);
    explorer.move_card(&child, Point::new(594.0, 0.0)).unwrap();
    explorer
        .move_card(&grandchild, Point::new(1188.0, 0.0))
        .unwrap();
    let before = explorer.session.clone();
    assert!(explorer.arrange_layout(None).unwrap());
    assert_eq!(card(&explorer, "root").position, Point::default());
    assert_eq!(card(&explorer, "child").position.x, 620.0);
    assert_eq!(card(&explorer, "grandchild").position.x, 1240.0);
    assert!(area(&explorer.session.cards) > area(&before.cards));
    assert_content_preserved(&explorer.session, &before);
    assert_nonoverlapping(&explorer.session.cards);
}

#[test]
fn shared_descendants_and_cycles_are_discovered_once_without_moving_unrelated_parents() {
    let mut explorer = connected_canvas();
    let root = card(&explorer, "root").id.clone();
    let earlier = card(&explorer, "earlier").id.clone();
    let grandchild = card(&explorer, "grandchild").id.clone();
    let unrelated = card(&explorer, "unrelated").id.clone();
    explorer.language.target = symbol("grandchild");
    explorer
        .expand_definition(&earlier, Position::new(0, 4))
        .unwrap();
    explorer
        .expand_definition(&unrelated, Position::new(0, 4))
        .unwrap();
    explorer.language.target = explorer.session.cards[0].source.symbol.clone();
    explorer
        .expand_references(&grandchild, Position::new(0, 4))
        .unwrap();
    let before = explorer.session.clone();
    assert!(explorer.arrange_layout(Some(&root)).unwrap());
    assert_eq!(
        card(&explorer, "unrelated").position,
        before.cards.last().unwrap().position
    );
    assert_content_preserved(&explorer.session, &before);
    assert_nonoverlapping(&explorer.session.cards);
    assert!(explorer.undo_layout().unwrap());
    assert_eq!(explorer.session.cards, before.cards);
}

#[test]
fn leaf_roots_and_single_or_empty_canvases_remain_unchanged() {
    let mut explorer = connected_canvas();
    let leaf = card(&explorer, "grandchild").id.clone();
    let before = explorer.session.clone();
    let candidate = explorer.plan_layout(Some(&leaf)).unwrap();
    assert!(!candidate.changed);
    assert!(!explorer.apply_layout_commit(candidate).unwrap());
    assert_eq!(explorer.session, before);
    let mut empty = super::explorer();
    assert!(!empty.arrange_layout(None).unwrap());
    empty
        .add_symbol(symbol("single"), Point::new(-300.0, 500.0))
        .unwrap();
    let before = empty.session.clone();
    assert!(!empty.arrange_layout(None).unwrap());
    assert_eq!(empty.session, before);
}

#[test]
fn unopened_empty_canvas_arrange_and_snapshot_planning_are_noops() {
    let original = explorer();
    let mut unopened = Explorer::new(original.language, Repository);
    let before = unopened.session.clone();
    assert!(!unopened.arrange_layout(None).unwrap());
    assert_eq!(unopened.session, before);
    let candidate = unopened.layout_snapshot().plan(None).unwrap();
    assert!(!candidate.changed);
    assert!(!unopened.apply_layout_commit(candidate).unwrap());
    assert_eq!(unopened.session, before);
    assert!(unopened.save_session(Path::new("unopened.json")).is_err());
}

#[test]
fn layout_undo_is_invalidated_by_add_remove_resize_or_move() {
    for operation in 0..4 {
        let mut explorer = connected_canvas();
        assert!(explorer.arrange_layout(None).unwrap());
        assert!(explorer.can_undo_layout());
        let root = explorer.session.cards[0].id.clone();
        match operation {
            0 => {
                explorer
                    .add_symbol(symbol("new"), Point::new(30000.0, 30000.0))
                    .unwrap();
            }
            1 => {
                explorer.remove_card(&root).unwrap();
            }
            2 => {
                let mut source = explorer.session.cards[0].source.clone();
                source.code = "x".repeat(180);
                explorer.replace_card_source(0, source).unwrap();
            }
            _ => {
                explorer
                    .move_card(&root, Point::new(-1000.0, -1000.0))
                    .unwrap();
            }
        }
        let before = explorer.session.clone();
        assert!(!explorer.can_undo_layout());
        assert!(!explorer.undo_layout().unwrap());
        assert_eq!(explorer.session, before);
    }
}

#[test]
fn stale_layout_candidate_is_rejected_and_planning_is_pure() {
    let mut explorer = connected_canvas();
    let before = explorer.session.clone();
    let candidate = explorer.plan_layout(None).unwrap();
    assert!(candidate.changed);
    assert_eq!(explorer.session, before);
    let root = explorer.session.cards[0].id.clone();
    explorer
        .move_card(&root, Point::new(-1000.0, 500.0))
        .unwrap();
    let current = explorer.session.clone();
    assert!(explorer.apply_layout_commit(candidate).is_err());
    assert_eq!(explorer.session, current);
}

#[test]
fn same_position_move_intent_invalidates_layout_undo_in_direct_and_prepared_paths() {
    for prepared in [false, true] {
        let mut explorer = connected_canvas();
        assert!(explorer.arrange_layout(None).unwrap());
        assert!(explorer.can_undo_layout());
        let target = explorer.session.cards[0].id.clone();
        let position = explorer.session.cards[0].position;
        let before = explorer.session.clone();
        if prepared {
            let edit = explorer.prepare_move_card(&target, position).unwrap();
            let commit = explorer.plan_prepared(edit).unwrap();
            assert_eq!(explorer.session, before);
            assert!(explorer.can_undo_layout());
            explorer.apply_commit(commit).unwrap();
        } else {
            explorer.move_card(&target, position).unwrap();
        }
        assert_eq!(explorer.session, before);
        assert!(!explorer.can_undo_layout());
        assert!(!explorer.undo_layout().unwrap());
        assert_eq!(explorer.session, before);
    }
}

#[test]
fn cancelled_layout_snapshot_preserves_live_state_and_layout_undo() {
    for cancel_after in [0, 3] {
        let mut explorer = connected_canvas();
        assert!(explorer.arrange_layout(None).unwrap());
        let snapshot = explorer.layout_snapshot();
        let before = explorer.session.clone();
        let checks = std::cell::Cell::new(0);
        let candidate = snapshot.plan_cancellable(None, &|| {
            let count = checks.get();
            checks.set(count + 1);
            count >= cancel_after
        });
        assert!(candidate.is_err());
        assert!(checks.get() > cancel_after);
        assert_eq!(explorer.session, before);
        assert!(explorer.can_undo_layout());
        assert!(explorer.undo_layout().unwrap());
    }
}
