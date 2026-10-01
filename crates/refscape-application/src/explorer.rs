//! Explorer state and use-case orchestration.

use crate::{
    Result,
    ports::{LanguageService, SessionRepository},
};
use refscape_canvas::{
    graph::descendant_cards,
    layout::{
        CardRect, LayoutRules, nearest_vacant_position, plan_resize, plan_restore_repair,
        plan_tree_arrangement_cancellable, source_anchor_y, source_dimensions, validate_layout,
    },
    regions::build_regions,
};
use refscape_model::{
    CodeCard, Connection, ConnectionKind, MAX_ZOOM, MIN_ZOOM, Point, Position, ProjectCrate,
    ProjectLanguage, ProjectOptions, Session, SourceDocument, SourceRange, Symbol, Theme, Viewport,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

mod cards;
mod editing;
mod navigation;
mod project;
mod rearrange;
mod viewport;

pub use editing::{CanvasEditOutcome, PreparedCanvasCommit, PreparedCanvasEdit};
pub use rearrange::{CanvasLayoutSnapshot, PreparedLayoutCommit};

/// Runtime-only versions; viewport changes do not affect these versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanvasGeneration {
    pub session: u64,
    pub content: u64,
    pub geometry: u64,
}

fn fresh_generation() -> CanvasGeneration {
    static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
    CanvasGeneration {
        session: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
        content: 0,
        geometry: 0,
    }
}

#[derive(Clone)]
struct LayoutUndo {
    generation: CanvasGeneration,
    positions: Vec<(String, Point)>,
}

/// Temporary selection, shared by all visible excerpts of the same document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableInspection {
    pub path: PathBuf,
    pub position: Position,
    pub highlights: Vec<SourceRange>,
    pub description: Option<String>,
}

pub struct Explorer<L: LanguageService, R: SessionRepository> {
    language: L,
    repository: R,
    session: Session,
    project_crates: Vec<ProjectCrate>,
    generation: CanvasGeneration,
    layout_undo: Option<LayoutUndo>,
}

impl<L: LanguageService, R: SessionRepository> Explorer<L, R> {
    pub fn new(language: L, repository: R) -> Self {
        Self {
            language,
            repository,
            session: Session::new(PathBuf::new()),
            project_crates: Vec::new(),
            generation: fresh_generation(),
            layout_undo: None,
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn generation(&self) -> CanvasGeneration {
        self.generation
    }

    fn content_changed(&mut self) {
        self.generation.content += 1;
        self.generation.geometry += 1;
        self.layout_undo = None;
    }

    fn geometry_changed(&mut self) {
        self.generation.geometry += 1;
        self.layout_undo = None;
    }

    fn reset_generation(&mut self) {
        self.generation = fresh_generation();
        self.layout_undo = None;
    }

    fn validate_canvas(&self) -> Result<()> {
        self.session.validate()?;
        validate_layout(&self.session.cards, LayoutRules::default())
    }

    fn card_for_symbol(&self, symbol: &Symbol) -> Option<&CodeCard> {
        self.session
            .cards
            .iter()
            .find(|c| same_symbol(&c.source.symbol, symbol))
    }

    fn insert_source(&mut self, source: SourceDocument, placement: CardRect) -> String {
        if let Some(card) = self.card_for_symbol(&source.symbol) {
            return card.id.clone();
        }
        let base_id = format!("card:{}", source.symbol.id);
        let mut id = base_id.clone();
        let mut suffix = 1;
        while self.session.cards.iter().any(|card| card.id == id) {
            id = format!("{base_id}:{suffix}");
            suffix += 1;
        }
        self.session.cards.push(CodeCard {
            id: id.clone(),
            source,
            position: placement.position,
            width: placement.width,
            height: placement.height,
        });
        id
    }

    fn rebuild_regions(&mut self) {
        self.session.regions = build_regions(
            &self.session.cards,
            &self.session.project_root,
            &self.project_crates,
        );
    }
}

fn same_symbol(a: &Symbol, b: &Symbol) -> bool {
    a.path == b.path && (a.id == b.id || (a.kind == b.kind && a.range == b.range))
}

#[cfg(test)]
mod tests;
