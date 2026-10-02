use refscape_model::{
    CardId, CodeCard, Connection, Position, ProjectOpenOptions, Region, SourceRange, Symbol, Theme,
    Viewport,
};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProjectState {
    #[default]
    Empty,
    Open {
        epoch: refscape_model::ProjectEpoch,
        options: refscape_model::ResolvedProjectOptions,
    },
}
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};
/// An immutable, cheaply shared read view. Source snapshots are shared by every clone.
#[derive(Clone, Debug, PartialEq)]
pub struct ApplicationSnapshot {
    pub project_root: PathBuf,
    pub project_options: ProjectOpenOptions,
    pub cards: Arc<Vec<CodeCard>>,
    pub connections: Arc<Vec<Connection>>,
    pub regions: Arc<Vec<Region>>,
    pub viewport: Viewport,
    pub theme: Theme,
}
impl ApplicationSnapshot {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            project_root,
            project_options: ProjectOpenOptions::default(),
            cards: Arc::default(),
            connections: Arc::default(),
            regions: Arc::default(),
            viewport: Viewport::default(),
            theme: Theme::default(),
        }
    }
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.project_root.as_os_str().is_empty() {
            return Err("Session project root is empty".into());
        }
        self.viewport.validate()?;
        self.theme.validate()?;
        let mut cards = HashSet::new();
        for card in self.cards.iter() {
            if card.id.is_empty() || !cards.insert(card.id.as_str()) {
                return Err("Card IDs must be nonempty and unique".into());
            }
            // CardSource can only be constructed through its validating boundary.
            // Revalidating immutable text here would make geometry edits scan it.
            card.validate_geometry()?;
        }
        let mut edges = HashSet::new();
        for edge in self.connections.iter() {
            if edge.id.is_empty() || !edges.insert(edge.id.as_str()) {
                return Err("Connection IDs must be nonempty and unique".into());
            }
            if !cards.contains(edge.from.as_str()) || !cards.contains(edge.to.as_str()) {
                return Err("Connection refers to a missing card".into());
            }
        }
        let mut regions = HashSet::new();
        for region in self.regions.iter() {
            if region.id.is_empty() || !regions.insert(&region.id) {
                return Err("Region IDs must be nonempty and unique".into());
            }
            if region
                .card_ids
                .iter()
                .any(|id| !cards.contains(id.as_str()))
            {
                return Err("Region refers to a missing card".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariableInspection {
    pub path: PathBuf,
    pub position: Position,
    pub highlights: Vec<SourceRange>,
    pub description: Option<String>,
}
/// Every index is rebuilt in the same commit; no reader sees intermediate topology.
#[derive(Default)]
pub(crate) struct CanvasStore {
    pub by_id: HashMap<CardId, usize>,
    pub incoming: HashMap<CardId, Vec<usize>>,
    pub outgoing: HashMap<CardId, Vec<usize>>,
    symbol_id: HashMap<(PathBuf, String), Vec<usize>>,
    symbol_range: HashMap<(PathBuf, String, Position, Position), Vec<usize>>,
}
impl CanvasStore {
    pub fn index(snapshot: &ApplicationSnapshot) -> Self {
        let mut result = Self::default();
        for (index, card) in snapshot.cards.iter().enumerate() {
            result.by_id.insert(card.id.clone(), index);
            let symbol = &card.source.symbol;
            result
                .symbol_id
                .entry((symbol.path.clone(), symbol.id.clone()))
                .or_default()
                .push(index);
            result
                .symbol_range
                .entry((
                    symbol.path.clone(),
                    symbol.kind.clone(),
                    symbol.range.start,
                    symbol.range.end,
                ))
                .or_default()
                .push(index);
        }
        for (index, edge) in snapshot.connections.iter().enumerate() {
            result
                .outgoing
                .entry(edge.from.clone())
                .or_default()
                .push(index);
            result
                .incoming
                .entry(edge.to.clone())
                .or_default()
                .push(index);
        }
        result
    }
    pub fn symbol<'a>(
        &self,
        snapshot: &'a ApplicationSnapshot,
        symbol: &Symbol,
    ) -> Option<&'a CodeCard> {
        let first_id = self
            .symbol_id
            .get(&(symbol.path.clone(), symbol.id.clone()))
            .and_then(|v| v.first());
        let first_range = self
            .symbol_range
            .get(&(
                symbol.path.clone(),
                symbol.kind.clone(),
                symbol.range.start,
                symbol.range.end,
            ))
            .and_then(|v| v.first());
        first_id
            .into_iter()
            .chain(first_range)
            .min()
            .map(|index| &snapshot.cards[*index])
    }
    pub fn card<'a>(&self, snapshot: &'a ApplicationSnapshot, id: &str) -> Option<&'a CodeCard> {
        self.by_id.get(id).map(|index| &snapshot.cards[*index])
    }
}
