use super::{CardRect, LayoutPlan, LayoutRules, nearest_vacant_position, validate_layout};
use crate::Result;
use refscape_model::CodeCard;

pub(crate) fn spatial_order(cards: &[CodeCard]) -> Vec<usize> {
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
    cards: &[CodeCard],
    target_id: &str,
    width: f32,
    height: f32,
    rules: LayoutRules,
) -> Result<LayoutPlan> {
    validate_layout(cards, rules)?;
    let target = cards
        .iter()
        .position(|card| card.id == target_id)
        .ok_or("Resize card does not exist")?;
    let resized = CardRect {
        position: cards[target].position,
        width,
        height,
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
        let rect = CardRect::from(&cards[index]);
        let placed = nearest_vacant_position(
            rect.position,
            rect.width,
            rect.height,
            None,
            &occupied,
            rules,
        )?;
        if placed.position != rect.position {
            changes.push(super::PositionChange {
                id: cards[index].id.clone(),
                before: rect.position,
                after: placed.position,
            });
        }
        occupied.push(placed);
    }
    for (i, a) in occupied.iter().enumerate() {
        if occupied[..i].iter().any(|b| a.overlaps_with(*b, rules)) {
            return Err("Invalid resize plan".into());
        }
    }
    Ok(LayoutPlan { changes })
}

/// Retain every initially clear card, then a maximal stable subset of overlap groups.
pub fn plan_restore_repair(cards: &[CodeCard], rules: LayoutRules) -> Result<LayoutPlan> {
    rules.validate()?;
    for card in cards {
        card.validate_geometry()?;
        CardRect::from(card).validate()?;
    }
    let colliding: Vec<_> = cards
        .iter()
        .enumerate()
        .map(|(i, card)| {
            cards.iter().enumerate().any(|(j, other)| {
                i != j && CardRect::from(card).overlaps_with(CardRect::from(other), rules)
            })
        })
        .collect();
    let order = spatial_order(cards);
    let mut occupied: Vec<_> = cards
        .iter()
        .enumerate()
        .filter(|(i, _)| !colliding[*i])
        .map(|(_, card)| CardRect::from(card))
        .collect();
    let mut move_indices = Vec::new();
    for index in order {
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
        let rect = CardRect::from(&cards[index]);
        let placed = nearest_vacant_position(
            rect.position,
            rect.width,
            rect.height,
            None,
            &occupied,
            rules,
        )?;
        candidate[index].position = placed.position;
        occupied.push(placed);
    }
    LayoutPlan::between(cards, &candidate, rules)
}
