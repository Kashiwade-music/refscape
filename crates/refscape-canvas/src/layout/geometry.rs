use super::LayoutCard;
use super::{CARD_GAP, CARD_RIGHT_GAP};
use crate::Result;
use refscape_model::Point;

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
            return Err(crate::invalid("Layout gaps must be finite and nonnegative"));
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

impl From<&LayoutCard> for CardRect {
    fn from(card: &LayoutCard) -> Self {
        Self {
            position: card.position.point(),
            width: card.size.width(),
            height: card.size.height(),
        }
    }
}

impl CardRect {
    pub fn overlaps(self, other: Self) -> bool {
        self.overlaps_with(other, LayoutRules::default())
    }

    pub fn overlaps_with(self, other: Self, rules: LayoutRules) -> bool {
        crate::instrumentation::overlap();
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
            return Err(crate::invalid(
                "Card placement exceeds finite canvas limits",
            ));
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

pub fn validate_layout(cards: &[LayoutCard], rules: LayoutRules) -> Result<()> {
    validate_layout_cancellable(cards, rules, &|| false)
}

pub(crate) fn validate_layout_cancellable(
    cards: &[LayoutCard],
    rules: LayoutRules,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    rules.validate()?;
    let mut ids = std::collections::BTreeSet::new();
    for (index, card) in cards.iter().enumerate() {
        super::check_cancelled(cancelled)?;
        card.validate_geometry()?;
        let rect = CardRect::from(card);
        rect.validate()?;
        if !ids.insert(&card.id) {
            return Err(crate::invalid("Duplicate card ID in layout"));
        }
        if card.id.is_empty() {
            return Err(crate::invalid("Layout card ID must be nonempty"));
        }
        for (offset, other) in cards[..index].iter().enumerate() {
            if offset % 64 == 0 {
                super::check_cancelled(cancelled)?;
            }
            if rect.overlaps_with(CardRect::from(other), rules) {
                return Err(crate::invalid("Cards overlap or violate the required gap"));
            }
        }
    }
    Ok(())
}
