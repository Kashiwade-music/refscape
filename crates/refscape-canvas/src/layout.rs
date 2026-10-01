//! Transactional card placement in world coordinates.

mod geometry;
mod nearest;
mod repair;
#[cfg(test)]
mod tests;
mod tree;

pub use geometry::{CardRect, LayoutRules, validate_layout};
pub use nearest::nearest_vacant_position;
pub use repair::{plan_resize, plan_restore_repair};
pub use tree::{plan_tree_arrangement, plan_tree_arrangement_cancellable};

use crate::Result;
use refscape_model::{
    CODE_CARD_HEADER, CODE_LINE_HEIGHT, CODE_REGION_HEADER, CODE_REGION_PADDING, CodeCard, Point,
    Position, SourceDocument,
};

pub const CARD_GAP: f32 = CODE_REGION_HEADER + CODE_REGION_PADDING + 16.0;
pub const CARD_RIGHT_GAP: f32 = 100.0;

pub fn source_anchor_y(card: &CodeCard, position: Position) -> f32 {
    card.position.y
        + CODE_CARD_HEADER
        + 8.0
        + card.source.display_anchor_row(position).unwrap_or(0) as f32 * CODE_LINE_HEIGHT
}

pub fn source_dimensions(source: &SourceDocument) -> (f32, f32) {
    let longest = source
        .display_lines()
        .iter()
        .map(|line| {
            line.text
                .chars()
                .map(|character| {
                    if character == '\t' {
                        4
                    } else if character.is_ascii() {
                        1
                    } else {
                        2
                    }
                })
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    (
        (longest as f32 * 8.0 + source.code_gutter_width() + 20.0).max(520.0),
        CodeCard::source_height(source),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionChange {
    pub id: String,
    pub before: Point,
    pub after: Point,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutPlan {
    pub changes: Vec<PositionChange>,
}

impl LayoutPlan {
    pub(crate) fn between(
        before: &[CodeCard],
        after: &[CodeCard],
        rules: LayoutRules,
    ) -> Result<Self> {
        validate_layout(after, rules)?;
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

    /// Verify the snapshot and the complete result before committing any positions.
    /// Resize callers first prepare the new source and dimensions in a private snapshot.
    pub fn apply_positions(&self, cards: &mut [CodeCard], rules: LayoutRules) -> Result<()> {
        let mut candidate = cards.to_vec();
        for change in &self.changes {
            let card = candidate
                .iter_mut()
                .find(|card| card.id == change.id)
                .ok_or("Layout card no longer exists")?;
            if card.position != change.before {
                return Err("Layout snapshot is stale".into());
            }
            card.position = change.after;
        }
        validate_layout(&candidate, rules)?;
        for (card, candidate) in cards.iter_mut().zip(candidate) {
            card.position = candidate.position;
        }
        Ok(())
    }
}
