//! Rectangle-aware ordered tree layout for the explicit Arrange action.

mod obstacles;
mod order;
#[cfg(test)]
mod tests;

use super::LayoutCard;
#[cfg(test)]
use super::validate_layout;
use super::{CardRect, LayoutDelta, LayoutRules, check_cancelled};
use crate::Result;
use order::OrderedTree;
use refscape_model::{Connection, Point};

pub fn plan_tree_arrangement(
    cards: &[LayoutCard],
    connections: &[Connection],
    selected: Option<&str>,
    rules: LayoutRules,
) -> Result<LayoutDelta> {
    plan_tree_arrangement_cancellable(cards, connections, selected, rules, &|| false)
}

/// Preserve the selected root (or first saved card) and every unreachable card.
/// Connections only select an ordered spanning tree; no connection is removed.
pub(crate) fn plan_tree_arrangement_cancellable(
    cards: &[LayoutCard],
    connections: &[Connection],
    selected: Option<&str>,
    rules: LayoutRules,
    cancelled: &dyn Fn() -> bool,
) -> Result<LayoutDelta> {
    check_cancelled(cancelled)?;
    super::geometry::validate_layout_cancellable(cards, rules, cancelled)?;
    if cards.is_empty() {
        if !connections.is_empty() {
            return Err(crate::invalid("Layout edge refers to a missing card"));
        }
        return LayoutDelta::between_cancellable(cards, cards, rules, cancelled);
    }
    let root = selected
        .and_then(|id| cards.iter().position(|card| card.id == id))
        .unwrap_or(0);
    let tree = OrderedTree::build(cards, connections, root, cancelled)?;
    if tree.order.len() == 1 {
        return LayoutDelta::between_cancellable(cards, cards, rules, cancelled);
    }
    let mut spacing_margin = 0.0_f64;
    // Translating to a coarser f32 grid may require wider subtree blocks, not just
    // another translation phase. Rebuild from the stable root and node sizes each
    // time; eight deterministic refinements bound work near representation limits.
    // No incomplete or unvalidated candidate is ever committed.
    for _ in 0..8 {
        check_cancelled(cancelled)?;
        let candidate = rectangle_positions(cards, &tree, rules, spacing_margin, cancelled)?;
        match obstacles::avoid_fixed(&candidate, &tree, rules, cancelled)? {
            obstacles::Placement::Ready(candidate) => {
                check_cancelled(cancelled)?;
                validate_tree(&candidate, &tree, rules)?;
                return LayoutDelta::between_cancellable(cards, &candidate, rules, cancelled);
            }
            obstacles::Placement::WiderSpacing(required) if required > spacing_margin => {
                spacing_margin = required
            }
            obstacles::Placement::WiderSpacing(_) => break,
        }
    }
    Err(crate::invalid(
        "Cannot preserve tree spacing at these canvas coordinates",
    ))
}

fn rectangle_positions(
    cards: &[LayoutCard],
    tree: &OrderedTree,
    rules: LayoutRules,
    spacing_margin: f64,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<LayoutCard>> {
    let rects: Vec<_> = cards.iter().map(CardRect::from).collect();
    let levels = tree.order.iter().map(|&i| tree.depth[i]).max().unwrap_or(0) + 1;
    let mut widths = vec![0.0_f64; levels];
    for &i in &tree.order {
        widths[tree.depth[i]] = widths[tree.depth[i]].max(f64::from(rects[i].width));
    }
    let mut xs = vec![cards[tree.root].position.x; levels];
    for depth in 1..levels {
        // Use the previous level's *saved* x, and add the gap after its right edge.
        // This preserves the rightward guarantee through both f64 cancellation and
        // f32 rounding, even when node widths differ within the same level.
        xs[depth] = ceil_f32(
            (f64::from(xs[depth - 1]) + widths[depth - 1])
                + f64::from(rules.right_gap.max(rules.gap)),
        )?;
    }
    let total_height: f64 = tree.order.iter().map(|&i| f64::from(rects[i].height)).sum();
    // A stable representational margin comes only from the fixed root and node
    // sizes, never old descendant positions. It prevents independent f32 row
    // rounding from consuming the requested gap and keeps repeated Arrange fixed.
    let scale = f64::from(cards[tree.root].position.y).abs()
        + total_height
        + f64::from(rules.gap) * tree.order.len() as f64;
    let gap = f64::from(rules.gap) + float_margin(scale)?.max(spacing_margin);
    let mut heights = vec![0.0_f64; cards.len()];
    let mut child_heights = vec![0.0_f64; cards.len()];
    for &i in tree.order.iter().rev() {
        let children = &tree.children[i];
        let children_height: f64 = children.iter().map(|&child| heights[child]).sum::<f64>()
            + children.len().saturating_sub(1) as f64 * gap;
        child_heights[i] = children_height;
        heights[i] = f64::from(rects[i].height).max(children_height);
    }
    let mut tops = vec![0.0_f64; cards.len()];
    tops[tree.root] = (f64::from(cards[tree.root].position.y)
        + f64::from(rects[tree.root].height) / 2.0)
        - heights[tree.root] / 2.0;
    let mut candidate = cards.to_vec();
    for &i in &tree.order {
        check_cancelled(cancelled)?;
        if i != tree.root {
            let y = tops[i] + (heights[i] - f64::from(rects[i].height)) / 2.0;
            candidate[i].position = Point::new(xs[tree.depth[i]], finite_f32(y)?).try_into()?;
            CardRect::from(&candidate[i]).validate()?;
        }
        let mut child_top = tops[i] + (heights[i] - child_heights[i]) / 2.0;
        for &child in &tree.children[i] {
            tops[child] = child_top;
            child_top += heights[child] + gap;
        }
    }
    // Validate the tree before considering external obstacles. Collision repair
    // translates its descendants as one group and cannot rearrange its branches.
    let reachable: Vec<_> = tree.order.iter().map(|&i| candidate[i].clone()).collect();
    super::geometry::validate_layout_cancellable(&reachable, rules, cancelled)?;
    validate_tree(&candidate, tree, rules)?;
    Ok(candidate)
}

fn validate_tree(cards: &[LayoutCard], tree: &OrderedTree, rules: LayoutRules) -> Result<()> {
    let mut last_at_depth =
        vec![None; tree.order.iter().map(|&i| tree.depth[i]).max().unwrap_or(0) + 1];
    for &i in &tree.order {
        if let Some(parent) = tree.parent[i]
            && f64::from(cards[i].position.x)
                < CardRect::from(&cards[parent]).right() + f64::from(rules.right_gap)
        {
            return Err(crate::invalid(
                "Tree children must remain to the right of their parent",
            ));
        }
        if let Some(previous) = last_at_depth[tree.depth[i]]
            && f64::from(cards[i].position.y)
                < CardRect::from(&cards[previous]).bottom() + f64::from(rules.gap)
        {
            return Err(crate::invalid(
                "Tree branches must retain source appearance order",
            ));
        }
        last_at_depth[tree.depth[i]] = Some(i);
    }
    Ok(())
}

fn finite_f32(value: f64) -> Result<f32> {
    let saved = value as f32;
    if !saved.is_finite() {
        return Err(crate::invalid("Tree layout exceeds finite canvas limits"));
    }
    Ok(saved)
}

fn ceil_f32(value: f64) -> Result<f32> {
    let saved = finite_f32(value)?;
    let saved = if f64::from(saved) < value {
        saved.next_up()
    } else {
        saved
    };
    if !saved.is_finite() {
        return Err(crate::invalid("Tree layout exceeds finite canvas limits"));
    }
    Ok(saved)
}

fn float_margin(scale: f64) -> Result<f64> {
    if !scale.is_finite() {
        return Err(crate::invalid("Tree layout exceeds finite canvas limits"));
    }
    // An unrepresentable far obstacle boundary must not discard its finite near
    // boundary. At the f32 limit use the inward ULP; clamp larger boundary scales
    // here and let each prospective saved card coordinate validate independently.
    let saved = scale.abs().min(f64::from(f32::MAX)) as f32;
    let margin = if saved.next_up().is_finite() {
        f64::from(saved.next_up()) - f64::from(saved)
    } else {
        f64::from(saved) - f64::from(saved.next_down())
    };
    Ok(margin * 4.0)
}
