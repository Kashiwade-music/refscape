use super::*;
use crate::layout::NodeOrderKey;
use refscape_model::{ConnectionKind, Position};
use std::cell::Cell;

fn card(id: &str, x: f32, y: f32, w: f32, h: f32) -> LayoutCard {
    LayoutCard {
        id: id.into(),
        position: Point::new(x, y).try_into().unwrap(),
        size: refscape_model::WorldSize::new(w, h).unwrap(),
        order: NodeOrderKey {
            path: format!("{id}.rs").into(),
            range_start: Position::default(),
            range_end: Position::default(),
            symbol_id: format!("file:{id}.rs"),
        },
    }
}
fn edge(from: &str, to: &str, line: u32, col: u32) -> Connection {
    Connection {
        id: format!("{from}:{to}:{line}:{col}").into(),
        from: from.into(),
        to: to.into(),
        source: Position::new(line, col),
        kind: ConnectionKind::Definition,
    }
}
fn arrange(cards: &[LayoutCard], edges: &[Connection], selected: Option<&str>) -> Vec<LayoutCard> {
    let rules = LayoutRules::default();
    let plan = plan_tree_arrangement(cards, edges, selected, rules).unwrap();
    let mut after = cards.to_vec();
    apply(&plan, &mut after, rules).unwrap();
    validate_layout(&after, rules).unwrap();
    after
}
fn find<'a>(cards: &'a [LayoutCard], id: &str) -> &'a LayoutCard {
    cards.iter().find(|c| c.id == id).unwrap()
}
fn right(cards: &[LayoutCard], from: &str, to: &str) {
    assert!(
        f64::from(find(cards, to).position.x)
            >= CardRect::from(find(cards, from)).right()
                + f64::from(LayoutRules::default().right_gap)
    );
}

#[test]
fn appearance_order_ignores_current_positions_and_target_names() {
    let cards = vec![
        card("root", 100.0, 50.0, 150.0, 128.0),
        card("a-last", 2000.0, -500.0, 100.0, 128.0),
        card("z-first", 1500.0, 500.0, 300.0, 400.0),
        card("middle", 3000.0, 0.0, 100.0, 200.0),
    ];
    let edges = vec![
        edge("root", "a-last", 20, 0),
        edge("root", "middle", 10, 8),
        edge("root", "z-first", 10, 2),
    ];
    let after = arrange(&cards, &edges, None);
    assert_eq!(after[0].position, cards[0].position);
    assert!(find(&after, "z-first").position.y < find(&after, "middle").position.y);
    assert!(find(&after, "middle").position.y < find(&after, "a-last").position.y);
    assert_eq!(after[1].position.x, after[2].position.x);
    assert_eq!(after[2].position.x, after[3].position.x);
    for child in ["z-first", "middle", "a-last"] {
        right(&after, "root", child);
    }
}

#[test]
fn rectangle_sizes_reserve_subtree_blocks_and_level_width() {
    let mut cards = vec![
        card("root", 0.0, 0.0, 100.0, 128.0),
        card("left", 1000.0, 0.0, 700.0, 200.0),
        card("right", 2000.0, 0.0, 100.0, 500.0),
        card("l1", 3000.0, 0.0, 100.0, 600.0),
        card("l2", 4000.0, 0.0, 100.0, 300.0),
        card("r1", 5000.0, 0.0, 100.0, 128.0),
    ];
    // Effective body height follows source even when the saved height is stale.
    cards[4].size = refscape_model::WorldSize::new(cards[4].size.width(), 676.0).unwrap();
    let edges = vec![
        edge("root", "left", 1, 0),
        edge("root", "right", 2, 0),
        edge("left", "l1", 1, 0),
        edge("left", "l2", 2, 0),
        edge("right", "r1", 1, 0),
    ];
    let after = arrange(&cards, &edges, None);
    assert!(find(&after, "l1").position.y < find(&after, "l2").position.y);
    assert!(
        f64::from(find(&after, "r1").position.y)
            >= CardRect::from(find(&after, "l2")).bottom() + f64::from(LayoutRules::default().gap)
    );
    assert_eq!(find(&after, "l1").position.x, find(&after, "r1").position.x);
    assert!(
        f64::from(find(&after, "r1").position.x)
            >= CardRect::from(find(&after, "left")).right() + 100.0
    );
    for (a, b) in [
        ("root", "left"),
        ("root", "right"),
        ("left", "l1"),
        ("left", "l2"),
        ("right", "r1"),
    ] {
        right(&after, a, b);
    }
}

#[test]
fn shared_child_primary_parent_uses_bfs_and_cycles_keep_all_connections() {
    let cards = vec![
        card("root", 0.0, 0.0, 100.0, 128.0),
        card("a", 1000.0, 0.0, 100.0, 128.0),
        card("b", 2000.0, 0.0, 100.0, 128.0),
        card("t", 3000.0, 0.0, 100.0, 128.0),
        card("shared", 4000.0, 0.0, 100.0, 128.0),
    ];
    let edges = vec![
        edge("root", "a", 1, 0),
        edge("root", "b", 2, 0),
        edge("a", "t", 1, 0),
        edge("b", "shared", 1, 0),
        edge("t", "shared", 1, 0),
        edge("shared", "root", 1, 0),
        edge("a", "a", 0, 0),
    ];
    let saved_edges = edges.clone();
    let after = arrange(&cards, &edges, None);
    assert_eq!(
        find(&after, "shared").position.x,
        find(&after, "t").position.x
    );
    assert!(find(&after, "t").position.y < find(&after, "shared").position.y);
    right(&after, "b", "shared");
    assert_eq!(edges, saved_edges);
}

#[test]
fn selected_subtree_only_moves_descendants_and_default_root_is_first_saved_card() {
    let cards = vec![
        card("first", 1000.0, 500.0, 100.0, 128.0),
        card("other", -5000.0, -2000.0, 100.0, 128.0),
        card("child", 2000.0, 0.0, 100.0, 128.0),
        card("grandchild", 3000.0, 0.0, 100.0, 128.0),
    ];
    let edges = vec![
        edge("first", "child", 1, 0),
        edge("child", "grandchild", 1, 0),
    ];
    let after = arrange(&cards, &edges, None);
    assert_eq!(after[0].position, cards[0].position);
    assert_eq!(after[1].position, cards[1].position);
    assert_eq!(arrange(&cards, &edges, Some("missing")), after);
    let selected = arrange(&cards, &edges, Some("child"));
    for i in [0, 1, 2] {
        assert_eq!(selected[i].position, cards[i].position);
    }
    assert_ne!(selected[3].position, cards[3].position);
    assert_eq!(arrange(&cards, &edges, Some("grandchild")), cards);
}

#[test]
fn fixed_obstacle_moves_all_descendants_together_without_reversing_branches() {
    let tree_cards = vec![
        card("root", 0.0, 0.0, 100.0, 128.0),
        card("first", 1000.0, 0.0, 100.0, 128.0),
        card("second", 2000.0, 0.0, 100.0, 128.0),
    ];
    let edges = vec![edge("root", "first", 1, 0), edge("root", "second", 2, 0)];
    let ideal = arrange(&tree_cards, &edges, None);
    let mut cards = tree_cards.clone();
    cards.push(card("obstacle", 200.0, -150.0, 100.0, 400.0));
    let after = arrange(&cards, &edges, None);
    assert_eq!(after[0].position, cards[0].position);
    assert_eq!(after[3].position, cards[3].position);
    let first_dy = after[1].position.y - ideal[1].position.y;
    let second_dy = after[2].position.y - ideal[2].position.y;
    assert!(first_dy != 0.0);
    assert!((first_dy - second_dy).abs() <= 0.0002);
    assert!(after[1].position.y < after[2].position.y);
    assert_eq!(arrange(&after, &edges, None), after);
}

#[test]
fn ordered_tree_is_adopted_even_when_bounding_box_grows() {
    let cards = vec![
        card("root", 0.0, 0.0, 100.0, 128.0),
        card("a", 0.0, 202.0, 100.0, 128.0),
        card("b", 0.0, 404.0, 100.0, 128.0),
        card("c", 0.0, 606.0, 100.0, 128.0),
    ];
    let edges = vec![
        edge("root", "a", 1, 0),
        edge("a", "b", 1, 0),
        edge("b", "c", 1, 0),
    ];
    let after = arrange(&cards, &edges, None);
    assert!(after[3].position.x >= 600.0);
    assert_eq!(after[3].position.y, 0.0);
    // This fixture grows from a vertical 100*734 box to a horizontal 700*128
    // box. Inspect the actual rectangles without reviving a production area API.
    let new_width = (after[3].position.x + after[3].size.width()) - after[0].position.x;
    let old_height = (cards[3].position.y + cards[3].size.height()) - cards[0].position.y;
    assert!(new_width * after[0].size.height() > cards[0].size.width() * old_height);
}

#[test]
fn repeated_arrange_and_connection_permutation_are_deterministic() {
    let cards = vec![
        card("root", 0.1, 0.1, 100.2, 128.0),
        card("z", 1000.0, 0.0, 200.2, 280.3),
        card("a", 2000.0, 0.0, 300.7, 500.1),
        card("b", 3000.0, 0.0, 100.1, 128.0),
    ];
    let edges = vec![
        edge("root", "z", 10, 0),
        edge("root", "a", 10, 0),
        edge("root", "b", 5, 0),
    ];
    let after = arrange(&cards, &edges, None);
    let mut reversed = edges.clone();
    reversed.reverse();
    assert_eq!(arrange(&cards, &reversed, None), after);
    assert_eq!(arrange(&after, &edges, None), after);
    assert!(find(&after, "b").position.y < find(&after, "a").position.y);
    assert!(find(&after, "a").position.y < find(&after, "z").position.y);
}

#[test]
fn empty_leaf_and_cancelled_plans_leave_input_unchanged() {
    assert!(
        plan_tree_arrangement(&[], &[], None, LayoutRules::default())
            .unwrap()
            .changes
            .is_empty()
    );
    let cards = vec![
        card("root", 0.0, 0.0, 100.0, 128.0),
        card("child", 1000.0, 0.0, 100.0, 128.0),
    ];
    let before = cards.clone();
    let edges = vec![edge("root", "child", 1, 0)];
    let calls = Cell::new(0);
    assert!(
        plan_tree_arrangement_cancellable(&cards, &edges, None, LayoutRules::default(), &|| {
            calls.set(calls.get() + 1);
            calls.get() >= 4
        })
        .is_err()
    );
    assert_eq!(cards, before);
    let plan = plan_tree_arrangement(&cards, &edges, None, LayoutRules::default()).unwrap();
    let mut stale = cards.clone();
    stale[1].position =
        refscape_model::WorldPoint::new(stale[1].position.x + 1.0, stale[1].position.y).unwrap();
    let snapshot = stale.clone();
    assert!(apply(&plan, &mut stale, LayoutRules::default()).is_err());
    assert_eq!(stale, snapshot);
}

#[test]
fn large_negative_root_adds_the_gap_after_right_edge() {
    let cards = vec![
        card("root", -1e20, 0.0, 1e20, 128.0),
        card("child", 1000.0, 0.0, 100.0, 128.0),
    ];
    let after = arrange(&cards, &[edge("root", "child", 1, 0)], None);
    assert_eq!(after[1].position.x, 100.0);
    right(&after, "root", "child");
}

fn coarse_translation_fixture(count: usize) -> (Vec<LayoutCard>, Vec<Connection>) {
    let mut cards = vec![card("root", 0.0, 0.0, 100.0, 128.0)];
    let mut edges = Vec::new();
    for i in 0..count {
        let id = format!("child-{i}");
        cards.push(card(&id, 1000.0 + i as f32 * 1000.0, 0.0, 100.0, 128.01));
        edges.push(edge("root", &id, i as u32, 0));
    }
    cards.push(card("obstacle", 200.0, -1e6, 100.0, 1.1e6));
    (cards, edges)
}

#[test]
fn many_translated_siblings_need_wider_spans_not_a_translation_phase() {
    let (cards, edges) = coarse_translation_fixture(7);
    let rules = LayoutRules::default();
    let tree = OrderedTree::build(&cards, &edges, 0, &|| false).unwrap();
    let base = rectangle_positions(&cards, &tree, rules, 0.0, &|| false).unwrap();
    let grid = f64::from(100000.0_f32.next_up()) - 100000.0;
    let required = f64::from(cards[1].size.height()) + f64::from(rules.gap);
    let minimum_span = 6.0 * (required / grid).ceil() * grid;
    let old_span = f64::from(base[7].position.y) - f64::from(base[1].position.y);
    // Endpoint rounding can change the total span by at most one grid unit. The
    // old span cannot hold six independently legal gaps at this destination scale.
    assert!(minimum_span > old_span + grid);
    let after = arrange(&cards, &edges, None);
    assert_eq!(after[0].position, cards[0].position);
    assert_eq!(after[8].position, cards[8].position);
    for pair in after[1..8].windows(2) {
        assert!(
            f64::from(pair[1].position.y)
                >= CardRect::from(&pair[0]).bottom() + f64::from(rules.gap)
        );
    }
    assert_eq!(arrange(&after, &edges, None), after);
}

#[test]
fn unrelated_far_obstacle_does_not_expand_tree_spacing() {
    let cards = vec![
        card("root", 0.0, 0.0, 100.0, 128.0),
        card("a", 1000.0, 0.0, 100.0, 128.01),
        card("b", 2000.0, 0.0, 100.0, 128.01),
    ];
    let edges = vec![edge("root", "a", 1, 0), edge("root", "b", 2, 0)];
    let expected = arrange(&cards, &edges, None);
    let mut with_far = cards.clone();
    with_far.push(card("far", 5000.0, 1e7, 100.0, 128.0));
    let after = arrange(&with_far, &edges, None);
    assert_eq!(&after[..3], expected.as_slice());
    assert_eq!(after[3], with_far[3]);
}

#[test]
fn unrepresentable_far_boundary_does_not_discard_finite_near_side() {
    let cards = vec![
        card("root", 0.0, 0.0, 100.0, 128.0),
        card("child", 1000.0, 0.0, 100.0, 128.0),
        card("obstacle", 200.0, -f32::MAX, 100.0, f32::MAX),
    ];
    let edges = vec![edge("root", "child", 1, 0)];
    let after = arrange(&cards, &edges, None);
    assert_eq!(after[0].position, cards[0].position);
    assert_eq!(after[2].position, cards[2].position);
    assert!(after[1].position.y >= 74.0 && after[1].position.y < 75.0);
    assert_eq!(arrange(&after, &edges, None), after);
}

#[test]
fn cancellation_during_spacing_rebuild_never_commits_partial_positions() {
    let (cards, edges) = coarse_translation_fixture(7);
    let calls = Cell::new(0);
    plan_tree_arrangement_cancellable(&cards, &edges, None, LayoutRules::default(), &|| {
        calls.set(calls.get() + 1);
        false
    })
    .unwrap();
    let checkpoints = calls.get();
    let before = cards.clone();
    // Exercise cancellation arriving at every checkpoint, including after the
    // obstacle repair requests a wider block layout, without assuming its count.
    for stop_at in 1..=checkpoints {
        let calls = Cell::new(0);
        assert!(
            plan_tree_arrangement_cancellable(
                &cards,
                &edges,
                None,
                LayoutRules::default(),
                &|| {
                    calls.set(calls.get() + 1);
                    calls.get() >= stop_at
                }
            )
            .is_err()
        );
        assert_eq!(cards, before);
    }
}
fn apply(plan: &LayoutDelta, cards: &mut [LayoutCard], rules: LayoutRules) -> Result<()> {
    let mut after = cards.to_vec();
    for change in &plan.changes {
        let card = after
            .iter_mut()
            .find(|c| c.id == change.id)
            .ok_or_else(|| crate::invalid("Missing card"))?;
        if card.position != change.before {
            return Err(crate::invalid("Stale plan"));
        }
        card.position = change.after;
    }
    validate_layout(&after, rules)?;
    cards.clone_from_slice(&after);
    Ok(())
}

#[test]
fn malformed_graph_input_is_rejected_with_typed_invalid_data() {
    let cards = vec![
        card("root", 0.0, 0.0, 100.0, 128.0),
        card("child", 1000.0, 0.0, 100.0, 128.0),
    ];
    let error = plan_tree_arrangement(
        &cards,
        &[edge("root", "missing", 0, 0)],
        None,
        LayoutRules::default(),
    )
    .unwrap_err();
    assert_eq!(error.kind, refscape_model::ErrorKind::InvalidData);
    assert_eq!(
        plan_tree_arrangement(
            &[],
            &[edge("root", "missing", 0, 0)],
            None,
            LayoutRules::default(),
        )
        .unwrap_err()
        .kind,
        refscape_model::ErrorKind::InvalidData
    );
    let mut invalid = cards.clone();
    invalid[0].id = "".into();
    assert_eq!(
        validate_layout(&invalid, LayoutRules::default())
            .unwrap_err()
            .kind,
        refscape_model::ErrorKind::InvalidData
    );
    assert_eq!(cards[0].position, Point::default());
}
