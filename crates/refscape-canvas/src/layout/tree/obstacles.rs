use super::{
    CardRect, LayoutRules, OrderedTree, check_cancelled, finite_f32, float_margin, validate_tree,
};
use crate::Result;
use crate::layout::LayoutCard;

pub(super) enum Placement {
    Ready(Vec<LayoutCard>),
    WiderSpacing(f64),
}

/// Translate all descendants together, preserving every branch and level order.
pub(super) fn avoid_fixed(
    base: &[LayoutCard],
    tree: &OrderedTree,
    rules: LayoutRules,
    cancelled: &dyn Fn() -> bool,
) -> Result<Placement> {
    if crate::layout::geometry::validate_layout_cancellable(base, rules, cancelled).is_ok() {
        return Ok(Placement::Ready(base.to_vec()));
    }
    let mut moving = vec![false; base.len()];
    for &i in &tree.order {
        moving[i] = i != tree.root;
    }
    let rects: Vec<_> = base.iter().map(CardRect::from).collect();
    let top = tree
        .order
        .iter()
        .map(|&i| f64::from(rects[i].position.y))
        .reduce(f64::min)
        .unwrap_or(0.0);
    let bottom = tree
        .order
        .iter()
        .map(|&i| rects[i].bottom())
        .reduce(f64::max)
        .unwrap_or(0.0);
    let span = bottom - top;
    let mut intervals = Vec::new();
    for &i in &tree.order {
        check_cancelled(cancelled)?;
        if !moving[i] {
            continue;
        }
        let rect = rects[i];
        let gap = f64::from(rules.gap);
        for (j, other) in rects.iter().enumerate() {
            if j % 64 == 0 {
                check_cancelled(cancelled)?;
            }
            if moving[j]
                || f64::from(rect.position.x) >= other.right() + gap
                || rect.right() + gap <= f64::from(other.position.x)
            {
                continue;
            }
            let lower = f64::from(other.position.y) - rect.bottom() - gap;
            let upper = other.bottom() + gap - f64::from(rect.position.y);
            // Pad only for f32 representation around the prospective boundary;
            // a huge far edge must not inflate the margin at a small near edge.
            let lower_margin = float_margin((f64::from(rect.position.y) + lower).abs() + span)?;
            let upper_margin = float_margin((f64::from(rect.position.y) + upper).abs() + span)?;
            intervals.push((lower - lower_margin, upper + upper_margin));
        }
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (lower, upper) in intervals {
        check_cancelled(cancelled)?;
        if let Some(last) = merged.last_mut()
            && lower < last.1
        {
            last.1 = last.1.max(upper);
        } else {
            merged.push((lower, upper));
        }
    }
    let (lower, upper) = merged
        .iter()
        .copied()
        .find(|(a, b)| *a < 0.0 && 0.0 < *b)
        .ok_or_else(|| crate::invalid("Cannot repair tree collisions with fixed cards"))?;
    let mut offsets = [lower, upper];
    offsets.sort_by(|a, b| {
        a.abs()
            .total_cmp(&b.abs())
            .then((a < &0.0).cmp(&(b < &0.0)))
    });
    let mut required_margin: Option<f64> = None;
    for dy in offsets {
        check_cancelled(cancelled)?;
        let mut candidate = base.to_vec();
        let mut finite = true;
        for &i in &tree.order {
            if !moving[i] {
                continue;
            }
            match finite_f32(f64::from(base[i].position.y) + dy) {
                Ok(y) => {
                    candidate[i].position =
                        refscape_model::WorldPoint::new(candidate[i].position.x, y)?
                }
                Err(_) => {
                    finite = false;
                    break;
                }
            }
        }
        if finite
            && crate::layout::geometry::validate_layout_cancellable(&candidate, rules, cancelled)
                .is_ok()
            && validate_tree(&candidate, tree, rules).is_ok()
        {
            return Ok(Placement::Ready(candidate));
        }
        if finite
            && tree
                .order
                .iter()
                .all(|&i| candidate[i].validate_geometry().is_ok())
        {
            let reachable: Vec<_> = tree.order.iter().map(|&i| candidate[i].clone()).collect();
            if crate::layout::geometry::validate_layout_cancellable(&reachable, rules, cancelled)
                .is_err()
                || validate_tree(&candidate, tree, rules).is_err()
            {
                // Only internal tree spacing can request a rebuild. Fixed-card
                // collisions and unrepresentable card extents remain placement
                // errors. Multiple siblings may have no valid translation phase
                // until their subtree spans grow on this coarser destination grid.
                let scale = tree
                    .order
                    .iter()
                    .filter(|&&i| moving[i])
                    .map(|&i| {
                        let rect = CardRect::from(&candidate[i]);
                        f64::from(rect.position.y).abs().max(rect.bottom().abs())
                    })
                    .reduce(f64::max)
                    .unwrap_or(0.0);
                let margin = float_margin(scale)?;
                required_margin = Some(required_margin.map_or(margin, |old| old.min(margin)));
            }
        }
    }
    if let Some(margin) = required_margin {
        return Ok(Placement::WiderSpacing(margin));
    }
    Err(crate::invalid(
        "Cannot place the ordered tree around fixed cards",
    ))
}
