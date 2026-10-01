//! Explorer state and use-case orchestration.

use crate::{
    Result,
    ports::{LanguageService, SessionRepository},
};
use refscape_canvas::{
    graph::descendant_cards,
    layout::{
        CARD_COLUMN_GAP, CARD_GAP, CardRect, arrange_cards, arrange_connected_cards, compact_cards,
        source_anchor_y, source_dimensions, vacant_position,
    },
    regions::build_regions,
};
use refscape_model::{
    CodeCard, Connection, ConnectionKind, MAX_ZOOM, MIN_ZOOM, Point, Position, ProjectCrate,
    ProjectLanguage, ProjectOptions, Session, SourceDocument, SourceRange, Symbol, Theme, Viewport,
};
use std::path::{Path, PathBuf};

mod cards;
mod navigation;
mod project;
mod viewport;

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
}

impl<L: LanguageService, R: SessionRepository> Explorer<L, R> {
    pub fn new(language: L, repository: R) -> Self {
        Self {
            language,
            repository,
            session: Session::new(PathBuf::new()),
            project_crates: Vec::new(),
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
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
