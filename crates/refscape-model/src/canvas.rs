use crate::{CardId, CardSource, EdgeId, Position, RefscapeError, WorldPoint, WorldSize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct CodeCard {
    pub id: CardId,
    pub source: CardSource,
    pub position: WorldPoint,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardMetrics {
    pub width: f32,
    pub height: f32,
    pub gutter_width: f32,
}
impl CardMetrics {
    pub fn world_size(self) -> Result<WorldSize, RefscapeError> {
        WorldSize::new(self.width, self.height)
    }
}

pub const CODE_CARD_HEADER: f32 = 52.0;
pub const CODE_LINE_HEIGHT: f32 = 20.0;
pub const CODE_REGION_PADDING: f32 = 22.0;
pub const CODE_REGION_HEADER: f32 = 36.0;

impl CodeCard {
    /// Dimensions and edges must remain usable in the stored world coordinates.
    pub fn validate_geometry(&self) -> Result<(), String> {
        WorldSize::new(self.width, self.height).map_err(|error| error.to_string())?;
        WorldSize::new(self.width, self.display_height())
            .and_then(|size| size.validate_at(self.position))
            .map_err(|error| error.to_string())
    }

    /// World-space height shared by painting and collision detection, including
    /// source that outgrew the dimensions stored in an older session.
    pub fn display_height(&self) -> f32 {
        self.height.max(Self::source_height(&self.source))
    }

    pub fn source_height(source: &CardSource) -> f32 {
        source.metrics().height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionKind {
    Definition,
    TypeDefinition,
    Reference,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub id: EdgeId,
    pub from: CardId,
    pub to: CardId,
    pub kind: ConnectionKind,
    pub source: Position,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
    pub card_ids: Vec<CardId>,
    pub kind: RegionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    Crate,
    Module,
    Project,
}
