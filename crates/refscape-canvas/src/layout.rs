//! Transactional card placement and column compaction in world coordinates.

use crate::Result;
use refscape_model::{
    CODE_CARD_HEADER, CODE_LINE_HEIGHT, CODE_REGION_HEADER, CODE_REGION_PADDING, CodeCard,
    Connection, Point, Position, SourceDocument,
};

// Include the visible file frame and its title in the space between rows.
pub const CARD_GAP: f32 = CODE_REGION_HEADER + CODE_REGION_PADDING + 16.0;
pub const CARD_COLUMN_GAP: f32 = 100.0;
const COLUMN_ALIGNMENT_TOLERANCE: f32 = 32.0;

pub fn source_anchor_y(card: &CodeCard, position: Position) -> f32 {
    card.position.y
        + CODE_CARD_HEADER
        + 8.0
        + position
            .line
            .saturating_sub(card.source.symbol.range.start.line) as f32
            * CODE_LINE_HEIGHT
}

#[derive(Clone, Copy)]
pub struct CardRect {
    pub position: Point,
    pub width: f32,
    pub height: f32,
}

impl From<&CodeCard> for CardRect {
    fn from(card: &CodeCard) -> Self {
        Self {
            position: card.position,
            width: card.width,
            height: card.display_height(),
        }
    }
}

impl CardRect {
    pub fn overlaps(self, other: Self) -> bool {
        self.position.x < other.position.x + other.width + CARD_GAP
            && self.position.x + self.width + CARD_GAP > other.position.x
            && self.position.y < other.position.y + other.height + CARD_GAP
            && self.position.y + self.height + CARD_GAP > other.position.y
    }

    fn validate(self) -> Result<()> {
        if !self.position.is_finite()
            || !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.0
            || self.height <= 0.0
            || !(self.position.x + self.width + CARD_GAP).is_finite()
            || !(self.position.y + self.height + CARD_GAP).is_finite()
        {
            return Err("Card placement exceeds finite canvas limits".into());
        }
        Ok(())
    }
}

pub fn source_dimensions(source: &SourceDocument) -> (f32, f32) {
    // This estimates display space only; source structure remains language-server supplied.
    let longest = source
        .code
        .lines()
        .map(|line| {
            line.chars()
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
    let width = (longest as f32 * 8.0 + 80.0).max(520.0);
    let height = CodeCard::source_height(source);
    (width, height)
}

/// Preserve clear positions and move colliding cards below occupied space.
/// Calculate every placement first so invalid geometry cannot partially reflow a canvas.
pub fn arrange_cards(cards: &mut [CodeCard]) -> Result<()> {
    let mut occupied = Vec::with_capacity(cards.len());
    for card in cards.iter() {
        occupied.push(vacant_position(
            card.position,
            card.width,
            card.display_height(),
            &occupied,
        )?);
    }
    for (card, rect) in cards.iter_mut().zip(occupied) {
        card.position = rect.position;
        card.height = rect.height;
    }
    Ok(())
}

/// Pack the remaining columns from the previous canvas origin after cards close.
/// Keep their horizontal column order and vertical reading order, using full source sizes.
pub fn compact_cards(
    cards: &mut [CodeCard],
    anchor: Point,
    connections: &[Connection],
) -> Result<()> {
    let mut order: Vec<_> = (0..cards.len()).collect();
    for card in cards.iter() {
        CardRect::from(card).validate()?;
    }
    order.sort_by(|&left, &right| {
        cards[left]
            .position
            .x
            .total_cmp(&cards[right].position.x)
            .then(cards[left].position.y.total_cmp(&cards[right].position.y))
            .then(cards[left].id.cmp(&cards[right].id))
    });
    let mut columns: Vec<Vec<usize>> = vec![];
    let mut column_left = f32::NEG_INFINITY;
    for index in order {
        let card = &cards[index];
        // Widths vary within a column. A wide lower card must not merge the
        // next column into this one just because their horizontal spans overlap.
        if columns.is_empty() || card.position.x >= column_left + COLUMN_ALIGNMENT_TOLERANCE {
            columns.push(vec![]);
            column_left = card.position.x;
        }
        columns
            .last_mut()
            .ok_or("missing layout column")?
            .push(index);
    }
    let mut placements: Vec<(usize, CardRect)> = Vec::with_capacity(cards.len());
    let mut x = anchor.x;
    for mut column in columns {
        column.sort_by(|&left, &right| {
            cards[left]
                .position
                .y
                .total_cmp(&cards[right].position.y)
                .then(cards[left].id.cmp(&cards[right].id))
        });
        let mut y = anchor.y;
        let mut width: f32 = 0.0;
        for index in column {
            let card = &cards[index];
            let linked_y = connections
                .iter()
                .filter(|edge| edge.to == card.id)
                .filter_map(|edge| {
                    let (parent_index, rect) = placements
                        .iter()
                        .find(|(parent_index, _)| cards[*parent_index].id == edge.from)?;
                    Some(
                        source_anchor_y(&cards[*parent_index], edge.source) + rect.position.y
                            - cards[*parent_index].position.y,
                    )
                })
                .reduce(f32::min);
            if let Some(linked_y) = linked_y {
                y = y.max(linked_y);
            }
            let rect = CardRect {
                position: Point::new(x, y),
                width: card.width,
                height: card.display_height(),
            };
            rect.validate()?;
            width = width.max(rect.width);
            y += rect.height + CARD_GAP;
            placements.push((index, rect));
        }
        x += width + CARD_COLUMN_GAP;
    }
    for (index, rect) in placements {
        cards[index].position = rect.position;
        cards[index].height = rect.height;
    }
    Ok(())
}

pub fn vacant_position(
    position: Point,
    width: f32,
    height: f32,
    occupied: &[CardRect],
) -> Result<CardRect> {
    let mut candidate = CardRect {
        position,
        width,
        height,
    };
    // Each step clears at least one occupied rectangle. The limit also protects
    // against coordinates so large that adding a card height loses precision.
    for _ in 0..=occupied.len() {
        candidate.validate()?;
        let next_y = occupied
            .iter()
            .filter(|other| candidate.overlaps(**other))
            .map(|other| other.position.y + other.height + CARD_GAP)
            .reduce(f32::max);
        match next_y {
            None => return Ok(candidate),
            Some(next_y) if next_y.is_finite() && next_y > candidate.position.y => {
                candidate.position.y = next_y;
            }
            Some(_) => return Err("Cannot place card at these canvas coordinates".into()),
        }
    }
    Err("Cannot find an unoccupied card position".into())
}
