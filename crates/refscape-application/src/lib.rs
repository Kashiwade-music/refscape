//! Application operations and ports for language services and persistence.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use refscape_model::{
    CodeCard, Connection, ConnectionKind, MAX_ZOOM, MIN_ZOOM, Point, Position, ProjectCrate,
    Region, Session, SourceDocument, SourceRange, Symbol, Theme, Viewport,
};

pub type Result<T> = std::result::Result<T, String>;

/// All structure and relationships come from a language's official backend.
pub trait LanguageService: Send {
    fn open_project(&mut self, root: &Path) -> Result<()>;
    fn files(&mut self) -> Result<Vec<PathBuf>>;
    fn symbols(&mut self, path: &Path) -> Result<Vec<Symbol>>;
    fn source(&mut self, symbol: &Symbol) -> Result<SourceDocument>;
    fn definitions(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>>;
    fn references(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>>;

    fn project_crates(&mut self) -> Result<Vec<ProjectCrate>> {
        Ok(Vec::new())
    }

    fn search(&mut self, query: &str) -> Result<Vec<Symbol>> {
        let query = query.to_lowercase();
        let mut found = Vec::new();
        for path in self.files()? {
            collect_matches(&self.symbols(&path)?, &query, &mut found);
        }
        Ok(found)
    }
}

pub trait SessionRepository: Send {
    fn save(&self, path: &Path, session: &Session) -> Result<()>;
    fn load(&self, path: &Path) -> Result<Session>;
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

    pub fn open_project(&mut self, root: &Path) -> Result<()> {
        let root = root
            .canonicalize()
            .map_err(|error| format!("Cannot open project {}: {error}", root.display()))?;
        if !root.is_dir() {
            return Err(format!(
                "Project root is not a directory: {}",
                root.display()
            ));
        }
        self.language.open_project(&root)?;
        let project_crates = self.language.project_crates()?;
        let theme = self.session.theme.clone();
        self.session = Session::new(root);
        self.session.theme = theme;
        self.project_crates = project_crates;
        Ok(())
    }

    pub fn files(&mut self) -> Result<Vec<PathBuf>> {
        self.language.files()
    }

    pub fn symbols(&mut self, path: &Path) -> Result<Vec<Symbol>> {
        self.language.symbols(path)
    }

    pub fn search(&mut self, query: &str) -> Result<Vec<Symbol>> {
        let mut found = self.language.search(query)?;
        found.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.path.cmp(&b.path))
                .then(a.range.start.cmp(&b.range.start))
        });
        Ok(found)
    }

    pub fn add_symbol(&mut self, symbol: Symbol, position: Point) -> Result<String> {
        if !position.is_finite() {
            return Err("Card position must be finite".into());
        }
        if let Some(card) = self.card_for_symbol(&symbol) {
            return Ok(card.id.clone());
        }
        let source = self.language.source(&symbol)?;
        source.validate()?;
        if let Some(card) = self.card_for_symbol(&source.symbol) {
            return Ok(card.id.clone());
        }
        let (width, height) = source_dimensions(&source);
        let occupied: Vec<_> = self.session.cards.iter().map(CardRect::from).collect();
        let placement = vacant_position(position, width, height, &occupied)?;
        let id = self.insert_source(source, placement);
        self.rebuild_regions();
        Ok(id)
    }

    pub fn add_file(&mut self, path: &Path, position: Point) -> Result<String> {
        self.add_symbol(
            Symbol::file(path.to_path_buf(), SourceRange::default()),
            position,
        )
    }

    pub fn expand_definition(&mut self, card_id: &str, position: Position) -> Result<Vec<String>> {
        self.expand(card_id, position, ConnectionKind::Definition)
    }

    pub fn expand_references(&mut self, card_id: &str, position: Position) -> Result<Vec<String>> {
        self.expand(card_id, position, ConnectionKind::Reference)
    }

    fn expand(
        &mut self,
        card_id: &str,
        position: Position,
        kind: ConnectionKind,
    ) -> Result<Vec<String>> {
        let origin = self
            .session
            .cards
            .iter()
            .find(|c| c.id == card_id)
            .cloned()
            .ok_or_else(|| format!("Unknown card {card_id}"))?;
        if !origin.source.symbol.range.contains(position) {
            return Err("Requested source position is outside the card".into());
        }
        let symbols = match kind {
            ConnectionKind::Definition => self
                .language
                .definitions(&origin.source.symbol.path, position)?,
            ConnectionKind::Reference => self
                .language
                .references(&origin.source.symbol.path, position)?,
        };
        // Resolve every source before changing the canvas, so a failed request
        // cannot leave an incomplete expansion behind.
        let mut sources = Vec::new();
        for symbol in symbols {
            symbol.validate()?;
            let existing = self.card_for_symbol(&symbol).map(|c| c.source.clone());
            let source = match existing {
                Some(source) => source,
                None => self.language.source(&symbol)?,
            };
            source.validate()?;
            if !sources
                .iter()
                .any(|existing: &SourceDocument| same_symbol(&existing.symbol, &source.symbol))
            {
                sources.push(source);
            }
        }
        let mut placements = Vec::new();
        let mut occupied: Vec<_> = self.session.cards.iter().map(CardRect::from).collect();
        let mut next_position =
            Point::new(origin.position.x + origin.width + 100.0, origin.position.y);
        for source in sources {
            let placement = match self.card_for_symbol(&source.symbol) {
                Some(existing) => CardRect::from(existing),
                None => {
                    let (width, height) = source_dimensions(&source);
                    let placement = vacant_position(next_position, width, height, &occupied)?;
                    occupied.push(placement);
                    next_position.y = placement.position.y + placement.height + CARD_GAP;
                    placement
                }
            };
            placements.push((source, placement));
        }
        let mut ids = Vec::new();
        for (source, placement) in placements {
            let target = self.insert_source(source, placement);
            if !ids.contains(&target) {
                ids.push(target.clone());
            }
            if !self.session.connections.iter().any(|c| {
                c.from == card_id && c.to == target && c.kind == kind && c.source == position
            }) {
                let mut number = self.session.connections.len();
                let id = loop {
                    let candidate = format!("connection:{number}");
                    if !self
                        .session
                        .connections
                        .iter()
                        .any(|connection| connection.id == candidate)
                    {
                        break candidate;
                    }
                    number += 1;
                };
                self.session.connections.push(Connection {
                    id,
                    from: card_id.into(),
                    to: target,
                    kind,
                    source: position,
                });
            }
        }
        self.rebuild_regions();
        Ok(ids)
    }

    pub fn move_card(&mut self, id: &str, position: Point) -> Result<()> {
        if !position.is_finite() {
            return Err("Card position must be finite".into());
        }
        let mut cards = self.session.cards.clone();
        cards
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("Unknown card {id}"))?
            .position = position;
        arrange_cards(&mut cards)?;
        self.session.cards = cards;
        Ok(())
    }

    pub fn remove_card(&mut self, id: &str) -> Result<()> {
        if !self.session.cards.iter().any(|c| c.id == id) {
            return Err(format!("Unknown card {id}"));
        }
        self.session.cards.retain(|c| c.id != id);
        self.session
            .connections
            .retain(|c| c.from != id && c.to != id);
        self.rebuild_regions();
        Ok(())
    }

    pub fn pan(&mut self, delta: Point) -> Result<()> {
        let offset = Point::new(
            self.session.viewport.offset.x + delta.x,
            self.session.viewport.offset.y + delta.y,
        );
        if !offset.is_finite() {
            return Err("Pan coordinates must be finite".into());
        }
        self.session.viewport.offset = offset;
        Ok(())
    }

    /// Keep the world point under the cursor fixed while changing scale.
    pub fn zoom(&mut self, factor: f32, screen_anchor: Point) -> Result<()> {
        if !factor.is_finite() || factor <= 0.0 || !screen_anchor.is_finite() {
            return Err("Zoom requires a finite positive scale and anchor".into());
        }
        let old = self.session.viewport;
        let world_anchor = old.screen_to_world(screen_anchor);
        let zoom = (old.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let viewport = Viewport {
            zoom,
            offset: Point::new(
                screen_anchor.x - world_anchor.x * zoom,
                screen_anchor.y - world_anchor.y * zoom,
            ),
        };
        viewport.validate()?;
        self.session.viewport = viewport;
        Ok(())
    }

    /// Apply UI-local movement as one validated transaction before a slow request.
    pub fn sync_canvas(
        &mut self,
        viewport: Viewport,
        positions: Vec<(String, Point)>,
    ) -> Result<()> {
        viewport.validate()?;
        for (id, position) in &positions {
            if !position.is_finite() {
                return Err("Card position must be finite".into());
            }
            if !self.session.cards.iter().any(|c| &c.id == id) {
                return Err(format!("Unknown card {id}"));
            }
        }
        let mut cards = self.session.cards.clone();
        for (id, position) in positions {
            if let Some(card) = cards.iter_mut().find(|c| c.id == id) {
                card.position = position;
            }
        }
        arrange_cards(&mut cards)?;
        self.session.viewport = viewport;
        self.session.cards = cards;
        Ok(())
    }

    pub fn set_theme(&mut self, theme: Theme) -> Result<()> {
        theme.validate()?;
        self.session.theme = theme;
        Ok(())
    }

    pub fn save_session(&self, path: &Path) -> Result<()> {
        self.session.validate()?;
        self.repository.save(path, &self.session)
    }

    pub fn load_session(&mut self, path: &Path) -> Result<()> {
        let mut session = self.repository.load(path)?;
        session.validate()?;
        arrange_cards(&mut session.cards)?;
        self.language.open_project(&session.project_root)?;
        let project_crates = self.language.project_crates()?;
        self.session = session;
        self.project_crates = project_crates;
        Ok(())
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
        let mut modules: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
        let mut crates: BTreeMap<String, (String, PathBuf, Vec<String>)> = BTreeMap::new();
        let mut unmatched = Vec::new();
        for card in &self.session.cards {
            modules
                .entry(card.source.symbol.path.clone())
                .or_default()
                .push(card.id.clone());
            match self
                .project_crates
                .iter()
                .filter(|project| card.source.symbol.path.starts_with(&project.root))
                .max_by_key(|project| project.root.components().count())
            {
                Some(project) => crates
                    .entry(project.id.clone())
                    .or_insert_with(|| (project.name.clone(), project.root.clone(), Vec::new()))
                    .2
                    .push(card.id.clone()),
                None => unmatched.push(card.id.clone()),
            }
        }
        self.session.regions = modules
            .into_iter()
            .map(|(path, card_ids)| Region {
                id: format!("module:{}", path.to_string_lossy()),
                label: path
                    .strip_prefix(&self.session.project_root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned(),
                path,
                card_ids,
            })
            .collect();
        for (id, (label, path, card_ids)) in crates.into_iter().rev() {
            self.session.regions.insert(
                0,
                Region {
                    id: format!("crate:{id}"),
                    label,
                    path,
                    card_ids,
                },
            );
        }
        if !unmatched.is_empty() {
            self.session.regions.insert(
                0,
                Region {
                    id: format!("project:{}", self.session.project_root.to_string_lossy()),
                    label: self
                        .session
                        .project_root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: self.session.project_root.clone(),
                    card_ids: unmatched,
                },
            );
        }
    }
}

const CARD_GAP: f32 = 32.0;

#[derive(Clone, Copy)]
struct CardRect {
    position: Point,
    width: f32,
    height: f32,
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
    fn overlaps(self, other: Self) -> bool {
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

fn source_dimensions(source: &SourceDocument) -> (f32, f32) {
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

fn vacant_position(
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

fn same_symbol(a: &Symbol, b: &Symbol) -> bool {
    a.path == b.path && (a.id == b.id || (a.kind == b.kind && a.range == b.range))
}

fn collect_matches(symbols: &[Symbol], query: &str, found: &mut Vec<Symbol>) {
    for symbol in symbols {
        if symbol.name.to_lowercase().contains(query) {
            found.push(symbol.clone());
        }
        collect_matches(&symbol.children, query, found);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Language {
        target: Symbol,
        fail: bool,
        code: String,
        additional: Vec<Symbol>,
        crates: Vec<ProjectCrate>,
    }
    impl LanguageService for Language {
        fn open_project(&mut self, _: &Path) -> Result<()> {
            Ok(())
        }
        fn files(&mut self) -> Result<Vec<PathBuf>> {
            Ok(vec![self.target.path.clone()])
        }
        fn symbols(&mut self, _: &Path) -> Result<Vec<Symbol>> {
            Ok(vec![self.target.clone()])
        }
        fn project_crates(&mut self) -> Result<Vec<ProjectCrate>> {
            Ok(self.crates.clone())
        }
        fn source(&mut self, symbol: &Symbol) -> Result<SourceDocument> {
            if self.fail {
                return Err("backend unavailable".into());
            }
            Ok(SourceDocument {
                symbol: symbol.clone(),
                code: self.code.clone(),
                tokens: Vec::new(),
            })
        }
        fn definitions(&mut self, _: &Path, _: Position) -> Result<Vec<Symbol>> {
            let mut targets = vec![self.target.clone(), self.target.clone()];
            targets.extend(self.additional.clone());
            Ok(targets)
        }
        fn references(&mut self, _: &Path, _: Position) -> Result<Vec<Symbol>> {
            Ok(vec![self.target.clone()])
        }
    }
    struct Repository;
    impl SessionRepository for Repository {
        fn save(&self, _: &Path, _: &Session) -> Result<()> {
            Ok(())
        }
        fn load(&self, _: &Path) -> Result<Session> {
            Err("no session".into())
        }
    }
    fn symbol(name: &str) -> Symbol {
        Symbol {
            id: name.into(),
            name: name.into(),
            kind: "function".into(),
            path: PathBuf::from(format!("/project/{name}.rs")),
            range: SourceRange {
                start: Position::default(),
                end: Position::new(0, 14),
            },
            selection_range: SourceRange {
                start: Position::new(0, 3),
                end: Position::new(0, 9),
            },
            children: Vec::new(),
        }
    }
    fn explorer() -> Explorer<Language, Repository> {
        let mut explorer = Explorer::new(
            Language {
                target: symbol("target"),
                fail: false,
                code: "fn target() {}".into(),
                additional: Vec::new(),
                crates: Vec::new(),
            },
            Repository,
        );
        explorer
            .open_project(&std::env::current_dir().unwrap())
            .unwrap();
        explorer
    }

    #[test]
    fn expansion_deduplicates_cards_and_edges_and_removal_cleans_regions() {
        let mut explorer = explorer();
        let origin = explorer
            .add_symbol(symbol("origin"), Point::default())
            .unwrap();
        explorer
            .expand_definition(&origin, Position::new(0, 4))
            .unwrap();
        explorer
            .expand_definition(&origin, Position::new(0, 4))
            .unwrap();
        assert_eq!(explorer.session.cards.len(), 2);
        assert_eq!(explorer.session.connections.len(), 1);
        assert_eq!(explorer.session.regions.len(), 3);
        let target = explorer.session.cards[1].id.clone();
        explorer.remove_card(&target).unwrap();
        assert!(explorer.session.connections.is_empty());
        assert_eq!(explorer.session.regions.len(), 2);
        assert!(explorer.session.validate().is_ok());
    }

    #[test]
    fn zoom_preserves_cursor_anchor_and_rejects_invalid_input() {
        let mut explorer = explorer();
        explorer.pan(Point::new(15.0, -22.0)).unwrap();
        let cursor = Point::new(400.0, 240.0);
        let world = explorer.session.viewport.screen_to_world(cursor);
        explorer.zoom(1.5, cursor).unwrap();
        assert_eq!(explorer.session.viewport.world_to_screen(world), cursor);
        let before = explorer.session.clone();
        assert!(explorer.zoom(f32::NAN, cursor).is_err());
        assert!(explorer.pan(Point::new(f32::INFINITY, 0.0)).is_err());
        assert_eq!(explorer.session, before);
    }

    #[test]
    fn failed_backend_expansion_and_sync_leave_canvas_unchanged() {
        let mut explorer = explorer();
        let origin = explorer
            .add_symbol(symbol("origin"), Point::default())
            .unwrap();
        let before = explorer.session.clone();
        explorer.language.fail = true;
        assert!(
            explorer
                .expand_definition(&origin, Position::new(0, 4))
                .is_err()
        );
        assert!(
            explorer
                .sync_canvas(
                    Viewport::default(),
                    vec![
                        (origin, Point::new(5.0, 5.0)),
                        ("missing".into(), Point::default())
                    ]
                )
                .is_err()
        );
        assert_eq!(explorer.session, before);
    }

    #[test]
    fn new_and_expanded_long_cards_do_not_overlap_or_clip_source() {
        let mut explorer = explorer();
        let line = "x".repeat(180);
        explorer.language.code = std::iter::repeat_n(line, 70).collect::<Vec<_>>().join("\n");
        explorer.language.additional.push(symbol("second_target"));
        let origin = explorer
            .add_symbol(symbol("origin"), Point::new(100.0, 80.0))
            .unwrap();
        let picker_card = explorer
            .add_symbol(symbol("picked"), Point::new(100.0, 80.0))
            .unwrap();
        let origin_rect = CardRect::from(&explorer.session.cards[0]);
        assert_eq!(origin_rect.width, 1520.0);
        assert_eq!(origin_rect.height, 1476.0);
        assert_ne!(
            explorer.session.cards[0].position,
            explorer.session.cards[1].position
        );
        // Occupy the natural first target location before expanding two tall sources.
        explorer
            .move_card(
                &picker_card,
                Point::new(
                    origin_rect.position.x + origin_rect.width + 100.0,
                    origin_rect.position.y,
                ),
            )
            .unwrap();
        let targets = explorer
            .expand_definition(&origin, Position::new(0, 4))
            .unwrap();
        assert_eq!(targets.len(), 2);
        assert_eq!(explorer.session.cards.len(), 4);
        for (index, card) in explorer.session.cards.iter().enumerate() {
            for other in &explorer.session.cards[index + 1..] {
                assert!(
                    !CardRect::from(card).overlaps(CardRect::from(other)),
                    "{} overlaps {}",
                    card.id,
                    other.id
                );
            }
        }
        explorer.session.validate().unwrap();
    }

    #[test]
    fn restore_and_canvas_sync_clear_full_source_heights_and_preserve_clear_cards() {
        let mut original = explorer();
        original.language.code = std::iter::repeat_n("fn tall() {}", 40)
            .collect::<Vec<_>>()
            .join("\n");
        for name in ["first", "second", "third", "clear"] {
            original.add_symbol(symbol(name), Point::default()).unwrap();
        }
        // An old snapshot used a fixed height and stacked cards using that height.
        for (index, card) in original.session.cards.iter_mut().enumerate() {
            card.height = 128.0;
            card.position = Point::new(0.0, index as f32 * 160.0);
        }
        let clear = Point::new(800.0, 20.0);
        original.session.cards[3].position = clear;
        let saved = original.session.clone();
        struct Saved(Session);
        impl SessionRepository for Saved {
            fn save(&self, _: &Path, _: &Session) -> Result<()> {
                Ok(())
            }
            fn load(&self, _: &Path) -> Result<Session> {
                Ok(self.0.clone())
            }
        }
        let mut restored = Explorer::new(original.language, Saved(saved));
        restored
            .load_session(Path::new("old-session.json"))
            .unwrap();
        let cards = &restored.session.cards;
        assert_eq!(cards[0].position, Point::default());
        assert_eq!(cards[0].height, 876.0);
        assert_eq!(cards[1].position.y, 908.0);
        assert_eq!(cards[2].position.y, 1816.0);
        assert_eq!(cards[3].position, clear);
        let before = cards.clone();
        arrange_cards(&mut restored.session.cards).unwrap();
        assert_eq!(restored.session.cards, before);

        // UI movement must use the same complete rectangles before expanding/saving.
        let positions = restored.session.cards[..3]
            .iter()
            .map(|card| (card.id.clone(), Point::default()))
            .collect();
        restored
            .sync_canvas(Viewport::default(), positions)
            .unwrap();
        for (index, card) in restored.session.cards.iter().enumerate() {
            for other in &restored.session.cards[index + 1..] {
                assert!(!CardRect::from(card).overlaps(CardRect::from(other)));
            }
        }
        assert_eq!(restored.session.cards[3].position, clear);
    }

    #[test]
    fn metadata_groups_workspace_packages_by_deepest_root() {
        let mut explorer = explorer();
        explorer.language.crates = vec![
            ProjectCrate {
                id: "outer".into(),
                name: "outer".into(),
                root: "/project".into(),
            },
            ProjectCrate {
                id: "nested".into(),
                name: "nested".into(),
                root: "/project/nested".into(),
            },
        ];
        explorer
            .open_project(&std::env::current_dir().unwrap())
            .unwrap();
        let outer = explorer
            .add_symbol(symbol("outer"), Point::default())
            .unwrap();
        let mut nested_symbol = symbol("nested");
        nested_symbol.path = "/project/nested/src/lib.rs".into();
        let nested = explorer
            .add_symbol(nested_symbol, Point::default())
            .unwrap();
        let regions = &explorer.session.regions;
        assert_eq!(
            regions
                .iter()
                .find(|region| region.id == "crate:outer")
                .unwrap()
                .card_ids,
            vec![outer]
        );
        assert_eq!(
            regions
                .iter()
                .find(|region| region.id == "crate:nested")
                .unwrap()
                .card_ids,
            vec![nested]
        );
        assert!(
            regions
                .iter()
                .all(|region| !region.id.starts_with("project:"))
        );
        explorer.session.validate().unwrap();
    }
}
