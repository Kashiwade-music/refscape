use super::*;

#[test]
fn expansion_deduplicates_cards_and_edges_and_removal_cleans_regions() {
    let mut explorer = explorer();
    let origin = explorer
        .add_symbol(symbol("origin"), Point::default())
        .unwrap();
    explorer
        .expand_definition(&origin, Position::new(0, 4))
        .unwrap();
    explorer
        .expand_definition(&origin, Position::new(0, 4))
        .unwrap();
    assert_eq!(explorer.session.cards.len(), 2);
    assert_eq!(explorer.session.connections.len(), 1);
    assert_eq!(explorer.session.regions.len(), 3);
    let target = explorer.session.cards[1].id.clone();
    explorer.remove_card(&target).unwrap();
    assert!(explorer.session.connections.is_empty());
    assert_eq!(explorer.session.regions.len(), 2);
    assert!(explorer.session.validate().is_ok());
}

#[test]
fn toggling_visible_symbols_cleans_links_and_reopens_the_card() {
    let mut explorer = explorer();
    let origin = explorer
        .add_symbol(symbol("origin"), Point::default())
        .unwrap();
    let target = explorer
        .toggle_definition(&origin, Position::new(0, 4))
        .unwrap()
        .unwrap()
        .remove(0);
    assert_eq!(explorer.session.cards.len(), 2);
    assert!(
        explorer
            .toggle_definition(&origin, Position::new(0, 4))
            .unwrap()
            .is_none()
    );
    assert_eq!(explorer.session.cards.len(), 1);
    assert!(explorer.session.connections.is_empty());
    assert_eq!(
        explorer
            .toggle_definition(&origin, Position::new(0, 4))
            .unwrap()
            .unwrap(),
        vec![target.clone()]
    );
    // A search symbol may carry a different opaque ID for the same source range.
    let mut selected = symbol("target");
    selected.id = "search-result-id".into();
    assert!(
        explorer
            .toggle_symbol(selected.clone(), Point::default())
            .unwrap()
            .is_none()
    );
    assert!(explorer.session.connections.is_empty());
    assert!(
        !explorer
            .session
            .regions
            .iter()
            .any(|region| region.card_ids.contains(&target))
    );
    assert!(
        explorer
            .toggle_symbol(selected, Point::default())
            .unwrap()
            .is_some()
    );
    assert_eq!(explorer.session.cards.len(), 2);
    explorer.session.validate().unwrap();
}

#[test]
fn reference_toggle_keeps_source_and_definition_toggle_hides_every_target() {
    let mut explorer = explorer();
    let origin = explorer
        .add_symbol(symbol("origin"), Point::default())
        .unwrap();
    explorer.language.additional.push(symbol("second_target"));
    explorer
        .toggle_definition(&origin, Position::new(0, 4))
        .unwrap();
    explorer.language.target = symbol("origin");
    explorer
        .toggle_references(&origin, Position::new(0, 4))
        .unwrap();
    assert!(
        explorer
            .toggle_references(&origin, Position::new(0, 4))
            .unwrap()
            .is_none()
    );
    assert_eq!(explorer.session.cards.len(), 3);
    assert!(
        explorer
            .toggle_definition(&origin, Position::new(0, 4))
            .unwrap()
            .is_none()
    );
    assert_eq!(explorer.session.cards.len(), 1);
    assert_eq!(explorer.session.cards[0].id, origin);
    assert!(explorer.session.connections.is_empty());
    explorer.session.validate().unwrap();
}

#[test]
fn closing_a_child_removes_its_descendants_for_every_close_action() {
    for action in 0..3 {
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
            .expand_references(&child, Position::new(0, 4))
            .unwrap()
            .remove(0);
        explorer.language.target = symbol("great_grandchild");
        explorer
            .expand_definition(&grandchild, Position::new(0, 4))
            .unwrap();
        match action {
            0 => explorer.remove_card(&child).unwrap(),
            1 => {
                explorer
                    .toggle_symbol(symbol("child"), Point::default())
                    .unwrap();
            }
            _ => {
                explorer
                    .toggle_definition(&root, Position::new(0, 4))
                    .unwrap();
            }
        }
        assert_eq!(explorer.session.cards.len(), 1);
        assert_eq!(explorer.session.cards[0].id, root);
        assert!(explorer.session.connections.is_empty());
        explorer.session.validate().unwrap();
    }
}

#[test]
fn closing_a_branch_preserves_shared_descendants_and_handles_cycles() {
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
    explorer.language.target = symbol("child");
    explorer
        .expand_definition(&grandchild, Position::new(0, 4))
        .unwrap();
    let other = explorer
        .add_symbol(symbol("other"), Point::new(0.0, 500.0))
        .unwrap();
    explorer.language.target = symbol("grandchild");
    explorer
        .expand_definition(&other, Position::new(0, 4))
        .unwrap();
    explorer.remove_card(&child).unwrap();
    assert_eq!(explorer.session.cards.len(), 3);
    assert!(
        explorer
            .session
            .cards
            .iter()
            .any(|card| card.id == grandchild)
    );
    explorer.remove_card(&other).unwrap();
    assert_eq!(explorer.session.cards.len(), 1);
    assert_eq!(explorer.session.cards[0].id, root);
    explorer.session.validate().unwrap();
}

#[test]
fn toggling_a_cyclic_branch_preserves_the_clicked_source() {
    let mut explorer = explorer();
    let root = explorer
        .add_symbol(symbol("root"), Point::default())
        .unwrap();
    explorer.language.target = symbol("child");
    let child = explorer
        .expand_definition(&root, Position::new(0, 4))
        .unwrap()
        .remove(0);
    explorer.language.target = symbol("root");
    explorer
        .expand_definition(&child, Position::new(0, 4))
        .unwrap();
    explorer
        .toggle_definition(&root, Position::new(0, 4))
        .unwrap();
    assert_eq!(explorer.session.cards.len(), 1);
    assert_eq!(explorer.session.cards[0].id, root);
    assert!(explorer.session.connections.is_empty());
    explorer.session.validate().unwrap();
}
