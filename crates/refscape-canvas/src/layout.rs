//! Pure placement planners over precomputed geometry, independent of source text.
mod geometry;
mod nearest;
mod operation;
mod repair;
#[cfg(test)]
mod tests;
mod tree;
use crate::Result;
pub use crate::metrics::{source_anchor_y, source_dimensions};
pub use geometry::{CardRect, LayoutRules, validate_layout};
pub use nearest::nearest_vacant_position;
pub use operation::{
    LayoutError, nearest_vacant_position_with_context, plan_arrange, plan_resize_with_context,
    plan_restore_repair_with_context, validate_layout_with_context,
};
use refscape_model::{CardId, CodeCard, Connection, Position, WorldPoint, WorldSize};
pub use repair::{plan_resize, plan_restore_repair};
use std::path::PathBuf;
pub use tree::plan_tree_arrangement;
pub const CARD_GAP: f32 = 74.0;
pub const CARD_RIGHT_GAP: f32 = 100.0;
/// Exact target ordering policy, detached from the source snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeOrderKey {
    pub path: PathBuf,
    pub range_start: Position,
    pub range_end: Position,
    pub symbol_id: String,
}
/// A saved-order node containing only geometry and stable ordering metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutCard {
    pub id: CardId,
    pub position: WorldPoint,
    pub size: WorldSize,
    pub order: NodeOrderKey,
}
impl LayoutCard {
    pub fn validate_geometry(&self) -> Result<()> {
        CardRect::from(self).validate()
    }
}
impl TryFrom<&CodeCard> for LayoutCard {
    type Error = refscape_model::RefscapeError;
    fn try_from(card: &CodeCard) -> Result<Self> {
        card.validate_geometry()
            .map_err(|error| crate::invalid(&error))?;
        let symbol = &card.source.symbol;
        Ok(Self {
            id: card.id.clone(),
            position: card.position,
            size: WorldSize::new(card.width, card.display_height())?,
            order: NodeOrderKey {
                path: symbol.path.clone(),
                range_start: symbol.range.start,
                range_end: symbol.range.end,
                symbol_id: symbol.id.clone(),
            },
        })
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutInput {
    pub cards: Vec<LayoutCard>,
    pub connections: Vec<Connection>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PositionChange {
    pub id: CardId,
    pub before: WorldPoint,
    pub after: WorldPoint,
}
/// Candidate geometry delta; application alone commits it after basis validation.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutDelta {
    pub changes: Vec<PositionChange>,
}
impl LayoutDelta {
    pub(crate) fn between_cancellable(
        before: &[LayoutCard],
        after: &[LayoutCard],
        rules: LayoutRules,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        if before.len() != after.len() || before.iter().zip(after).any(|(a, b)| a.id != b.id) {
            return Err(crate::invalid("Layout node set changed while planning"));
        }
        geometry::validate_layout_cancellable(after, rules, cancelled)?;
        Ok(Self {
            changes: before
                .iter()
                .zip(after)
                .filter(|(a, b)| a.position != b.position)
                .map(|(a, b)| PositionChange {
                    id: a.id.clone(),
                    before: a.position,
                    after: b.position,
                })
                .collect(),
        })
    }
}

pub(crate) fn check_cancelled(cancelled: &dyn Fn() -> bool) -> Result<()> {
    crate::instrumentation::checkpoint();
    if cancelled() {
        return Err(refscape_model::RefscapeError::new(
            refscape_model::ErrorKind::Cancelled,
            "Layout calculation cancelled",
        ));
    }
    Ok(())
}
