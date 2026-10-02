use super::*;
use refscape_model::{Point, Position};

fn rect(x: f32, y: f32, w: f32, h: f32) -> CardRect {
    CardRect {
        position: Point::new(x, y),
        width: w,
        height: h,
    }
}
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
fn zero_gap() -> LayoutRules {
    LayoutRules {
        gap: 0.0,
        right_gap: 0.0,
    }
}

fn nearest(p: Point, w: f32, h: f32, minimum: Option<f64>, occupied: &[CardRect]) -> CardRect {
    nearest_vacant_position(p, w, h, minimum, occupied, zero_gap()).unwrap()
}

#[test]
fn vacant_point_unchanged_and_obstacle_order_deterministic() {
    let obstacles = [rect(0.0, 0.0, 4.0, 8.0), rect(4.0, 3.0, 6.0, 5.0)];
    assert_eq!(
        nearest(Point::new(-10.0, -10.0), 2.0, 2.0, None, &obstacles).position,
        Point::new(-10.0, -10.0)
    );
    let a = nearest(Point::new(3.0, 4.0), 2.0, 2.0, None, &obstacles);
    assert_eq!(
        a,
        nearest(
            Point::new(3.0, 4.0),
            2.0,
            2.0,
            None,
            &[obstacles[1], obstacles[0]]
        )
    );
}

#[test]
fn right_halfplane_explores_above_below_and_further_right() {
    let obstacle = [rect(5.0, -2.0, 8.0, 4.0)];
    assert_eq!(
        nearest(Point::new(0.0, 0.0), 2.0, 2.0, Some(5.0), &obstacle).position,
        Point::new(5.0, 2.0)
    );
    assert_eq!(
        nearest(Point::new(0.0, -1.5), 2.0, 2.0, Some(5.0), &obstacle).position,
        Point::new(5.0, -4.0)
    );
    assert_eq!(
        nearest(Point::new(12.0, 0.0), 2.0, 2.0, Some(5.0), &obstacle).position,
        Point::new(13.0, 0.0)
    );
}

#[test]
fn touching_open_intervals_leave_a_legal_single_point() {
    let obstacles = [rect(0.0, -4.0, 20.0, 4.0), rect(0.0, 2.0, 20.0, 4.0)];
    assert_eq!(
        nearest(Point::new(10.0, 1.0), 2.0, 2.0, None, &obstacles).position,
        Point::new(10.0, 0.0)
    );
}

#[test]
fn nonrepresentable_touching_hole_does_not_hide_next_outer_boundary() {
    let u = f32::EPSILON;
    let obstacles = [
        rect(-100.0, -u / 2.0, 200.0, 1.0 + 2.0 * u),
        rect(-100.0, 1.0 + 2.0 * u, 200.0, 2.0 * u),
    ];
    let desired = Point::new(0.0, 1.0 + u);
    let actual = nearest(desired, 1.0, u / 2.0, None, &obstacles);
    // Independently enumerate every nearby saved y, including the unrepresentable
    // contact's neighboring floats. This oracle does not build forbidden intervals.
    let mut best = None;
    for k in -8..=12 {
        let r = rect(0.0, 1.0 + k as f32 * u, 1.0, u / 2.0);
        if r.validate().is_err() || obstacles.iter().any(|o| r.overlaps_with(*o, zero_gap())) {
            continue;
        }
        let dy = f64::from(r.position.y) - f64::from(desired.y);
        let rank = (dy.abs(), dy < 0.0);
        if best.is_none_or(|(old, _)| rank < old) {
            best = Some((rank, r.position));
        }
    }
    assert_eq!(actual.position, best.unwrap().1);
    assert_eq!(actual.position, Point::new(0.0, 1.0 + 5.0 * u));
}

#[test]
fn exact_minimum_x_with_lost_extent_uses_valid_adjacent_float() {
    let u = f32::EPSILON;
    let actual = nearest(
        Point::new(1.0 + u, 0.0),
        u / 2.0,
        1.0,
        Some(f64::from(1.0 + 2.0 * u)),
        &[],
    );
    assert_eq!(actual.position, Point::new(1.0 + 3.0 * u, 0.0));
    actual.validate().unwrap();
}

#[test]
fn exact_gap_contact_and_fractional_rounding_are_valid() {
    let rules = LayoutRules {
        gap: 0.1,
        right_gap: 0.1,
    };
    let obstacle = [rect(1.1, -10.0, 0.3, 20.0)];
    let min_x = f64::from(obstacle[0].position.x) - f64::from(0.2_f32) - f64::from(rules.gap);
    let placed = nearest_vacant_position(
        Point::new(min_x as f32, 0.0),
        0.2,
        1.0,
        Some(min_x),
        &obstacle,
        rules,
    )
    .unwrap();
    assert!(f64::from(placed.position.x) >= min_x);
    assert!(!placed.overlaps_with(obstacle[0], rules));
    let obstacle = [rect(0.1, 0.1, 0.2, 0.2)];
    let placed =
        nearest_vacant_position(Point::new(0.2, 0.2), 0.2, 0.2, None, &obstacle, rules).unwrap();
    assert!(!placed.overlaps_with(obstacle[0], rules));
}

#[test]
fn independent_integer_oracle_matches_nearest_distance_and_ties() {
    // Forbidden edges and desired coordinates are integers, so the exact optimum
    // lies on this independent grid; it does not reuse candidate generation.
    for seed in 0..64 {
        let obstacles = [
            rect((seed % 5 - 2) as f32, (seed % 7 - 3) as f32, 2.0, 3.0),
            rect((seed % 9 - 4) as f32, (seed % 3 - 1) as f32, 3.0, 2.0),
        ];
        let desired = Point::new((seed % 7 - 3) as f32, (seed % 5 - 2) as f32);
        let minimum = if seed % 2 == 0 { Some(0.0) } else { None };
        let actual = nearest(desired, 2.0, 2.0, minimum, &obstacles);
        let mut best = None;
        for x in -20..=20 {
            for y in -20..=20 {
                if minimum.is_some_and(|min| f64::from(x) < min) {
                    continue;
                }
                let r = rect(x as f32, y as f32, 2.0, 2.0);
                if obstacles.iter().any(|o| {
                    r.position.x < o.position.x + o.width
                        && r.position.x + r.width > o.position.x
                        && r.position.y < o.position.y + o.height
                        && r.position.y + r.height > o.position.y
                }) {
                    continue;
                }
                let dx = f64::from(x) - f64::from(desired.x);
                let dy = f64::from(y) - f64::from(desired.y);
                let rank = (dx * dx + dy * dy, dy.abs(), dy < 0.0, x, y);
                if best.is_none_or(|(old, _)| rank < old) {
                    best = Some((rank, r.position));
                }
            }
        }
        assert_eq!(actual.position, best.unwrap().1, "seed {seed}");
    }
}

#[test]
fn invalid_obstacle_and_lost_extent_rejected() {
    assert!(
        nearest_vacant_position(
            Point::default(),
            2.0,
            2.0,
            None,
            &[rect(50.0, 0.0, 2.0, 2.0), rect(f32::NAN, 0.0, 2.0, 2.0)],
            zero_gap()
        )
        .is_err()
    );
    assert!(rect(1e30, 0.0, 1.0, 1.0).validate().is_err());
    assert!(rect(f32::MAX, 0.0, f32::MAX, 1.0).validate().is_err());
    assert!(refscape_model::WorldSize::new(10.0, f32::NAN).is_err());
}

#[test]
fn resize_moves_direct_hits_only_and_shrink_moves_nobody() {
    let mut cards = vec![
        card("target", 0.0, 0.0, 100.0, 128.0),
        card("hit", 0.0, 202.0, 100.0, 128.0),
        card("fixed", 0.0, 500.0, 100.0, 128.0),
    ];
    let plan = plan_resize(
        &cards,
        "target",
        refscape_model::WorldSize::new(100.0, 250.0).unwrap(),
        LayoutRules::default(),
    )
    .unwrap();
    assert_eq!(
        plan.changes
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        vec!["hit"]
    );
    cards[0].size = refscape_model::WorldSize::new(100.0, 250.0).unwrap();
    apply(&plan, &mut cards, LayoutRules::default()).unwrap();
    assert_eq!(cards[0].position, Point::default());
    assert_eq!(cards[2].position, Point::new(0.0, 500.0));
    assert!(
        plan_resize(
            &cards,
            "target",
            refscape_model::WorldSize::new(100.0, 128.0).unwrap(),
            LayoutRules::default()
        )
        .unwrap()
        .changes
        .is_empty()
    );
}

#[test]
fn restore_preserves_initially_clear_card_and_is_idempotent() {
    let mut cards = vec![
        card("first", 0.0, 0.0, 100.0, 128.0),
        card("overlap", 0.0, 0.0, 100.0, 128.0),
        card("clear", 0.0, 202.0, 100.0, 128.0),
    ];
    let plan = plan_restore_repair(&cards, LayoutRules::default()).unwrap();
    apply(&plan, &mut cards, LayoutRules::default()).unwrap();
    assert_eq!(cards[2].position, Point::new(0.0, 202.0));
    assert!(
        plan_restore_repair(&cards, LayoutRules::default())
            .unwrap()
            .changes
            .is_empty()
    );
}

#[test]
fn bounded_nearest_respects_upper_limit_and_safe_rounding() {
    let obstacles = [rect(-2.0, -2.0, 4.0, 4.0)];
    let placed = super::nearest::nearest_vacant_position_bounded(
        Point::new(10.0, 0.0),
        1.0,
        1.0,
        Some(-4.0),
        Some(-3.0),
        &obstacles,
        zero_gap(),
    )
    .unwrap();
    assert_eq!(placed.position, Point::new(-3.0, 0.0));
    let limit = f64::from(0.1_f32) + f64::from(0.2_f32);
    let placed = super::nearest::nearest_vacant_position_bounded(
        Point::new(10.0, 0.0),
        1.0,
        1.0,
        None,
        Some(limit),
        &[],
        zero_gap(),
    )
    .unwrap();
    assert!(f64::from(placed.position.x) <= limit);
    assert!(
        super::nearest::nearest_vacant_position_bounded(
            Point::default(),
            1.0,
            1.0,
            Some(2.0),
            Some(1.0),
            &[],
            zero_gap()
        )
        .is_err()
    );
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
fn cancellation_at_every_nearest_resize_restore_checkpoint_preserves_inputs() {
    use std::cell::Cell;
    let clear = vec![
        card("target", 0.0, 0.0, 100.0, 128.0),
        card("hit", 0.0, 202.0, 100.0, 128.0),
        card("fixed", 0.0, 500.0, 100.0, 128.0),
    ];
    let overlap = vec![
        card("first", 0.0, 0.0, 100.0, 128.0),
        card("second", 0.0, 0.0, 100.0, 128.0),
        card("clear", 0.0, 202.0, 100.0, 128.0),
    ];
    let occupied = [rect(0.0, 0.0, 100.0, 128.0), rect(0.0, 202.0, 100.0, 128.0)];
    let run = |cancelled: &dyn Fn() -> bool| -> Result<()> {
        super::nearest::nearest_vacant_position_cancellable(
            Point::new(1.0, 1.0),
            100.0,
            128.0,
            None,
            None,
            &occupied,
            LayoutRules::default(),
            cancelled,
        )?;
        super::repair::plan_resize_cancellable(
            &clear,
            "target",
            refscape_model::WorldSize::new(100.0, 250.0).unwrap(),
            LayoutRules::default(),
            cancelled,
        )?;
        super::repair::plan_restore_repair_cancellable(
            &overlap,
            LayoutRules::default(),
            cancelled,
        )?;
        Ok(())
    };
    let count = Cell::new(0);
    run(&|| {
        count.set(count.get() + 1);
        false
    })
    .unwrap();
    let before = (clear.clone(), overlap.clone(), occupied);
    for stop in 1..=count.get() {
        let calls = Cell::new(0);
        assert!(
            run(&|| {
                calls.set(calls.get() + 1);
                calls.get() >= stop
            })
            .is_err()
        );
        assert_eq!(
            (&clear, &overlap, &occupied),
            (&before.0, &before.1, &before.2)
        );
    }
}

#[test]
fn operation_failures_keep_cancellation_and_timeout_typed() {
    use refscape_model::{ErrorKind, OperationContext};
    use std::time::Duration;
    let operation = OperationContext::detached(Duration::from_secs(60));
    operation.cancel.cancel();
    assert_eq!(
        plan_restore_repair_with_context(&[], LayoutRules::default(), &operation)
            .unwrap_err()
            .kind,
        ErrorKind::Cancelled
    );
    assert_eq!(
        plan_arrange(
            &LayoutInput {
                cards: vec![],
                connections: vec![]
            },
            None,
            LayoutRules::default(),
            &operation
        )
        .unwrap_err()
        .kind,
        ErrorKind::Cancelled
    );
    assert_eq!(
        nearest_vacant_position_with_context(
            Point::default(),
            refscape_model::WorldSize::new(10.0, 10.0).unwrap(),
            None,
            &[],
            LayoutRules::default(),
            &operation
        )
        .unwrap_err()
        .kind,
        ErrorKind::Cancelled
    );
    let expired = OperationContext::detached(Duration::ZERO);
    assert_eq!(
        validate_layout_with_context(&[], LayoutRules::default(), &expired)
            .unwrap_err()
            .kind,
        ErrorKind::Timeout
    );
}
