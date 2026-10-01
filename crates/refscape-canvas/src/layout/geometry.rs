use super::{CARD_GAP, CARD_RIGHT_GAP};
use crate::Result;
use refscape_model::{CodeCard, Point};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutRules {
    pub gap: f32,
    pub right_gap: f32,
}

impl Default for LayoutRules {
    fn default() -> Self {
        Self {
            gap: CARD_GAP,
            right_gap: CARD_RIGHT_GAP,
        }
    }
}

impl LayoutRules {
    pub fn validate(self) -> Result<()> {
        if !self.gap.is_finite()
            || !self.right_gap.is_finite()
            || self.gap < 0.0
            || self.right_gap < 0.0
        {
            return Err("Layout gaps must be finite and nonnegative".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
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
        self.overlaps_with(other, LayoutRules::default())
    }

    pub fn overlaps_with(self, other: Self, rules: LayoutRules) -> bool {
        let gap = f64::from(rules.gap);
        f64::from(self.position.x) < other.right() + gap
            && self.right() + gap > f64::from(other.position.x)
            && f64::from(self.position.y) < other.bottom() + gap
            && self.bottom() + gap > f64::from(other.position.y)
    }

    pub fn validate(self) -> Result<()> {
        if !self.position.is_finite()
            || !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.0
            || self.height <= 0.0
            || !(self.position.x + self.width).is_finite()
            || !(self.position.y + self.height).is_finite()
            || self.position.x + self.width <= self.position.x
            || self.position.y + self.height <= self.position.y
        {
            return Err("Card placement exceeds finite canvas limits".into());
        }
        Ok(())
    }

    pub(crate) fn right(self) -> f64 {
        f64::from(self.position.x) + f64::from(self.width)
    }
    pub(crate) fn bottom(self) -> f64 {
        f64::from(self.position.y) + f64::from(self.height)
    }
}

pub fn validate_layout(cards: &[CodeCard], rules: LayoutRules) -> Result<()> {
    rules.validate()?;
    let mut ids = std::collections::BTreeSet::new();
    for (index, card) in cards.iter().enumerate() {
        card.validate_geometry()?;
        let rect = CardRect::from(card);
        rect.validate()?;
        if !ids.insert(&card.id) {
            return Err("Duplicate card ID in layout".into());
        }
        if cards[..index]
            .iter()
            .any(|other| rect.overlaps_with(CardRect::from(other), rules))
        {
            return Err("Cards overlap or violate the required gap".into());
        }
    }
    Ok(())
}
