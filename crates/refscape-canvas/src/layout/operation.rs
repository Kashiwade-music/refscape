//! Operation-scoped planner entry points and typed failure boundary.
use super::{CardRect, LayoutCard, LayoutDelta, LayoutInput, LayoutRules};
use refscape_model::{OperationContext, Point, RefscapeError, WorldSize};
pub type LayoutError = RefscapeError;

fn execute<T>(
    operation: &OperationContext,
    plan: impl FnOnce(&dyn Fn() -> bool) -> crate::Result<T>,
) -> Result<T, LayoutError> {
    operation.check()?;
    let result = plan(&|| operation.check().is_err());
    operation.check()?;
    result
}

pub fn validate_layout_with_context(
    cards: &[LayoutCard],
    rules: LayoutRules,
    operation: &OperationContext,
) -> Result<(), LayoutError> {
    execute(operation, |cancelled| {
        super::geometry::validate_layout_cancellable(cards, rules, cancelled)
    })
}
pub fn plan_arrange(
    input: &LayoutInput,
    root: Option<&str>,
    rules: LayoutRules,
    operation: &OperationContext,
) -> Result<LayoutDelta, LayoutError> {
    execute(operation, |cancelled| {
        super::tree::plan_tree_arrangement_cancellable(
            &input.cards,
            &input.connections,
            root,
            rules,
            cancelled,
        )
    })
}
pub fn plan_resize_with_context(
    cards: &[LayoutCard],
    target: &str,
    size: WorldSize,
    rules: LayoutRules,
    operation: &OperationContext,
) -> Result<LayoutDelta, LayoutError> {
    execute(operation, |cancelled| {
        super::repair::plan_resize_cancellable(cards, target, size, rules, cancelled)
    })
}
pub fn plan_restore_repair_with_context(
    cards: &[LayoutCard],
    rules: LayoutRules,
    operation: &OperationContext,
) -> Result<LayoutDelta, LayoutError> {
    execute(operation, |cancelled| {
        super::repair::plan_restore_repair_cancellable(cards, rules, cancelled)
    })
}
pub fn nearest_vacant_position_with_context(
    position: Point,
    size: WorldSize,
    min_x: Option<f64>,
    occupied: &[CardRect],
    rules: LayoutRules,
    operation: &OperationContext,
) -> Result<CardRect, LayoutError> {
    execute(operation, |cancelled| {
        super::nearest::nearest_vacant_position_cancellable(
            position,
            size.width(),
            size.height(),
            min_x,
            None,
            occupied,
            rules,
            cancelled,
        )
    })
}
