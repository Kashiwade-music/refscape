//! Bounded glyph residency and immutable scene identity.
use gpui::{ShapedLine, TextRun, Window, px};
use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DetailLevel {
    Region,
    Summary,
    Code,
}
impl DetailLevel {
    pub(super) fn from_zoom(zoom: f32) -> Self {
        if zoom < 0.35 {
            Self::Region
        } else if zoom < 0.65 {
            Self::Summary
        } else {
            Self::Code
        }
    }
}
/// The initial policy intentionally preserves the established low-zoom labels.
pub(super) struct BackendPresentation {
    region_label: &'static str,
}
impl BackendPresentation {
    pub(super) fn for_language(language: refscape_model::ProjectLanguage) -> Self {
        Self {
            region_label: if language == refscape_model::ProjectLanguage::Cpp {
                "PROJECT"
            } else {
                "CRATES"
            },
        }
    }
    pub(super) fn detail_label(&self, detail: DetailLevel) -> &'static str {
        match detail {
            DetailLevel::Region => self.region_label,
            DetailLevel::Summary => "MODULES",
            DetailLevel::Code => "CODE",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ShapeKey {
    source: u64,
    folds: u64,
    row: usize,
    zoom: u32,
    device_scale: u32,
    style: String,
}
const ROW_CACHE_CAPACITY: usize = 2048;
/// Work performed by the last canvas frame, independent of elapsed time or GPU.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct FrameWork {
    pub(super) visible_rows: usize,
    pub(super) edge_rows: usize,
    pub(super) summary_metric_reads: usize,
    pub(super) shape_misses: usize,
    pub(super) painted_edges: usize,
}
#[derive(Default)]
pub(super) struct SceneCache {
    rows: HashMap<ShapeKey, (ShapedLine, ShapedLine)>,
    order: VecDeque<ShapeKey>,
    pub(super) shaped_rows: u64,
    pub(super) frame_work: FrameWork,
    pub(super) evicted_rows: u64,
    cards: HashMap<refscape_model::CardId, usize>,
    topology: Option<std::sync::Arc<Vec<refscape_model::CodeCard>>>,
    edges: Option<std::sync::Arc<Vec<refscape_model::Connection>>>,
    titles: HashMap<refscape_model::CardId, String>,
    source_keys: Vec<(
        refscape_model::CardId,
        refscape_model::SourceRevision,
        refscape_model::FoldRevision,
    )>,
}
impl SceneCache {
    pub(super) fn begin_frame(&mut self) {
        self.frame_work = FrameWork::default();
    }
    #[cfg(test)]
    pub(super) fn resident_rows(&self) -> usize {
        self.rows.len()
    }
    pub(super) fn clear(&mut self) {
        self.rows.clear();
        self.order.clear();
        self.cards.clear();
        self.topology = None;
        self.edges = None;
        self.titles.clear();
        self.source_keys.clear();
    }
    pub(super) fn index(&mut self, snapshot: &refscape_application::ApplicationSnapshot) {
        if self
            .topology
            .as_ref()
            .is_some_and(|cards| std::sync::Arc::ptr_eq(cards, &snapshot.cards))
            && self
                .edges
                .as_ref()
                .is_some_and(|edges| std::sync::Arc::ptr_eq(edges, &snapshot.connections))
        {
            return;
        }
        if !self
            .topology
            .as_ref()
            .is_some_and(|cards| std::sync::Arc::ptr_eq(cards, &snapshot.cards))
        {
            self.cards = snapshot
                .cards
                .iter()
                .enumerate()
                .map(|(index, card)| (card.id.clone(), index))
                .collect();
            self.topology = Some(snapshot.cards.clone());
        }
        self.index_titles(snapshot);
    }
    fn index_titles(&mut self, snapshot: &refscape_application::ApplicationSnapshot) {
        let keys: Vec<_> = snapshot
            .cards
            .iter()
            .map(|card| {
                let projection = card.source.projection();
                (
                    card.id.clone(),
                    projection.source_revision,
                    projection.fold_revision,
                )
            })
            .collect();
        if self
            .edges
            .as_ref()
            .is_some_and(|edges| std::sync::Arc::ptr_eq(edges, &snapshot.connections))
            && self.source_keys == keys
        {
            return;
        }
        self.edges = Some(snapshot.connections.clone());
        self.source_keys = keys;
        self.titles = snapshot
            .cards
            .iter()
            .map(|card| (card.id.clone(), card.source.symbol.name.clone()))
            .collect();
        let mut names: HashMap<refscape_model::CardId, Vec<String>> = HashMap::new();
        for edge in snapshot
            .connections
            .iter()
            .filter(|edge| edge.kind == refscape_model::ConnectionKind::TypeDefinition)
        {
            let Some(origin) = self
                .card(&edge.from)
                .and_then(|index| snapshot.cards.get(index))
            else {
                continue;
            };
            let Some((row, span)) =
                refscape_application::navigation::source_word(&origin.source, edge.source)
            else {
                continue;
            };
            let Some(row) = origin.source.projection().rows.get(row) else {
                continue;
            };
            let name = row.text()[span].to_owned();
            let variables = names.entry(edge.to.clone()).or_default();
            if !variables.contains(&name) {
                variables.push(name);
            }
        }
        for (id, variables) in names {
            if let Some(title) = self.titles.get_mut(&id) {
                *title = format!("{} → {}", variables.join(", "), title);
            }
        }
    }
    pub(super) fn title(&self, card: &refscape_model::CodeCard) -> String {
        self.titles
            .get(&card.id)
            .cloned()
            .unwrap_or_else(|| card.source.symbol.name.clone())
    }
    pub(super) fn card(&self, id: &refscape_model::CardId) -> Option<usize> {
        self.cards.get(id).copied()
    }
    pub(super) fn shape(
        &mut self,
        source: &refscape_model::CardSource,
        row: usize,
        runs: &[TextRun],
        zoom: f32,
        window: &mut Window,
    ) -> (ShapedLine, ShapedLine) {
        let style = runs
            .iter()
            .map(|run| format!("{}:{:?}:{:?};", run.len, run.font, run.color))
            .collect::<String>();
        let key = ShapeKey {
            source: source.projection().source_revision.0,
            folds: source.projection().fold_revision.0,
            row,
            zoom: zoom.to_bits(),
            device_scale: window.scale_factor().to_bits(),
            style,
        };
        if let Some(shapes) = self.rows.get(&key) {
            return shapes.clone();
        }
        let text = source.projection().rows[row].text();
        let world = window
            .text_system()
            .shape_line(text.to_owned().into(), px(12.0), runs, None);
        let device =
            window
                .text_system()
                .shape_line(text.to_owned().into(), px(12.0 * zoom), runs, None);
        let shapes = (world, device);
        self.shaped_rows += 1;
        self.frame_work.shape_misses += 1;
        self.rows.insert(key.clone(), shapes.clone());
        self.order.push_back(key);
        if self.rows.len() > ROW_CACHE_CAPACITY
            && let Some(key) = self.order.pop_front()
        {
            self.rows.remove(&key);
            self.evicted_rows += 1;
        }
        shapes
    }
}

#[cfg(test)]
mod tests {
    use super::DetailLevel;
    #[test]
    fn detail_boundaries_keep_code_summary_and_region_contract() {
        assert_eq!(DetailLevel::from_zoom(0.349_999), DetailLevel::Region);
        assert_eq!(DetailLevel::from_zoom(0.35), DetailLevel::Summary);
        assert_eq!(DetailLevel::from_zoom(0.649_999), DetailLevel::Summary);
        assert_eq!(DetailLevel::from_zoom(0.65), DetailLevel::Code);
    }
}
