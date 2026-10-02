use super::geometry::validate_layout_cancellable;
use super::nearest::nearest_vacant_position_cancellable;
use super::{CardRect, LayoutDelta, LayoutRules, check_cancelled};
use crate::Result;
use crate::layout::LayoutCard;
use refscape_model::WorldSize;

pub(crate) fn spatial_order(cards: &[LayoutCard]) -> Vec<usize> {
    let mut order: Vec<_> = (0..cards.len()).collect();
    order.sort_by(|&a, &b| {
        cards[a]
            .position
            .y
            .total_cmp(&cards[b].position.y)
            .then(cards[a].position.x.total_cmp(&cards[b].position.x))
            .then(cards[a].id.cmp(&cards[b].id))
    });
    order
}

/// Only rectangles directly hit by the resized card may move.
pub fn plan_resize(
    cards: &[LayoutCard],
    target_id: &str,
    size: WorldSize,
    rules: LayoutRules,
) -> Result<LayoutDelta> {
    plan_resize_cancellable(cards, target_id, size, rules, &|| false)
}

pub(crate) fn plan_resize_cancellable(
    cards: &[LayoutCard],
    target_id: &str,
    size: WorldSize,
    rules: LayoutRules,
    cancelled: &dyn Fn() -> bool,
) -> Result<LayoutDelta> {
    validate_layout_cancellable(cards, rules, cancelled)?;
    let target = cards
        .iter()
        .position(|card| card.id == target_id)
        .ok_or_else(|| crate::invalid("Resize card does not exist"))?;
    let resized = CardRect {
        position: cards[target].position.point(),
        width: size.width(),
        height: size.height(),
    };
    resized.validate()?;
    let displaced: Vec<_> = spatial_order(cards)
        .into_iter()
        .filter(|&i| i != target && resized.overlaps_with(CardRect::from(&cards[i]), rules))
        .collect();
    let mut occupied: Vec<_> = cards
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != target && !displaced.contains(i))
        .map(|(_, card)| CardRect::from(card))
        .collect();
    occupied.push(resized);
    let mut changes = Vec::new();
    for index in displaced {
        check_cancelled(cancelled)?;
        let rect = CardRect::from(&cards[index]);
        let placed = nearest_vacant_position_cancellable(
            rect.position,
            rect.width,
            rect.height,
            None,
            None,
            &occupied,
            rules,
            cancelled,
        )?;
        if placed.position != rect.position {
            changes.push(super::PositionChange {
                id: cards[index].id.clone(),
                before: cards[index].position,
                after: placed.position.try_into()?,
            });
        }
        occupied.push(placed);
    }
    for (i, a) in occupied.iter().enumerate() {
        check_cancelled(cancelled)?;
        for (j, b) in occupied[..i].iter().enumerate() {
            if j % 64 == 0 {
                check_cancelled(cancelled)?;
            }
            if a.overlaps_with(*b, rules) {
                return Err(crate::invalid("Invalid resize plan"));
            }
        }
    }
    Ok(LayoutDelta { changes })
}

/// Retain every initially clear card, then a maximal stable subset of overlap groups.
pub fn plan_restore_repair(cards: &[LayoutCard], rules: LayoutRules) -> Result<LayoutDelta> {
    plan_restore_repair_cancellable(cards, rules, &|| false)
}

pub(crate) fn plan_restore_repair_cancellable(
    cards: &[LayoutCard],
    rules: LayoutRules,
    cancelled: &dyn Fn() -> bool,
) -> Result<LayoutDelta> {
    check_cancelled(cancelled)?;
    rules.validate()?;
    for card in cards {
        check_cancelled(cancelled)?;
        card.validate_geometry()?;
        CardRect::from(card).validate()?;
    }
    let mut colliding = vec![false; cards.len()];
    for (i, card) in cards.iter().enumerate() {
        check_cancelled(cancelled)?;
        for (j, other) in cards.iter().enumerate() {
            if j % 64 == 0 {
                check_cancelled(cancelled)?;
            }
            if i != j && CardRect::from(card).overlaps_with(CardRect::from(other), rules) {
                colliding[i] = true;
                break;
            }
        }
    }
    let order = spatial_order(cards);
    let mut occupied: Vec<_> = cards
        .iter()
        .enumerate()
        .filter(|(i, _)| !colliding[*i])
        .map(|(_, card)| CardRect::from(card))
        .collect();
    let mut move_indices = Vec::new();
    for index in order {
        check_cancelled(cancelled)?;
        if !colliding[index] {
            continue;
        }
        let rect = CardRect::from(&cards[index]);
        if occupied
            .iter()
            .any(|other| rect.overlaps_with(*other, rules))
        {
            move_indices.push(index);
        } else {
            occupied.push(rect);
        }
    }
    let mut candidate = cards.to_vec();
    for index in move_indices {
        check_cancelled(cancelled)?;
        let rect = CardRect::from(&cards[index]);
        let placed = nearest_vacant_position_cancellable(
            rect.position,
            rect.width,
            rect.height,
            None,
            None,
            &occupied,
            rules,
            cancelled,
        )?;
        candidate[index].position = placed.position.try_into()?;
        occupied.push(placed);
    }
    LayoutDelta::between_cancellable(cards, &candidate, rules, cancelled)
}
