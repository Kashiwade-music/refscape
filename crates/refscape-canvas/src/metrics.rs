//! The source-to-metrics boundary. Geometry planners never inspect source rows.
use refscape_model::{CODE_CARD_HEADER, CODE_LINE_HEIGHT, CardSource, CodeCard, Position};
pub fn source_anchor_y(card: &CodeCard, position: Position) -> f32 {
    card.position.y
        + CODE_CARD_HEADER
        + 8.0
        + card.source.display_anchor_row(position).unwrap_or(0) as f32 * CODE_LINE_HEIGHT
}
pub fn source_dimensions(source: &CardSource) -> (f32, f32) {
    crate::instrumentation::metrics();
    let metrics = source.metrics();
    (metrics.width, metrics.height)
}
