//! Application operations and ports for language services and persistence.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use refscape_model::{
    CODE_CARD_HEADER, CODE_LINE_HEIGHT, CODE_REGION_HEADER, CODE_REGION_PADDING, CodeCard,
    Connection, ConnectionKind, MAX_ZOOM, MIN_ZOOM, Point, Position, ProjectCrate, Region, Session,
    SourceDocument, SourceRange, Symbol, Theme, Viewport,
};

pub type Result<T> = std::result::Result<T, String>;

/// Temporary selection, shared by all visible excerpts of the same document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableInspection {
    pub path: PathBuf,
    pub position: Position,
    pub highlights: Vec<SourceRange>,
    pub description: Option<String>,
}

/// All structure and relationships come from a language's official backend.
pub trait LanguageService: Send {
    fn open_project(&mut self, root: &Path) -> Result<()>;
    fn files(&mut self) -> Result<Vec<PathBuf>>;
    fn symbols(&mut self, path: &Path) -> Result<Vec<Symbol>>;
    fn source(&mut self, symbol: &Symbol) -> Result<SourceDocument>;
    fn definitions(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>>;
    fn references(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>>;

    fn type_definitions(&mut self, _path: &Path, _position: Position) -> Result<Vec<Symbol>> {
        Ok(Vec::new())
    }

    fn document_highlights(
        &mut self,
        _path: &Path,
        _position: Position,
    ) -> Result<Vec<SourceRange>> {
        Ok(Vec::new())
    }

    fn hover(&mut self, _path: &Path, _position: Position) -> Result<Option<String>> {
        Ok(None)
    }

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

    pub fn hover(&mut self, card_id: &str, position: Position) -> Result<Option<String>> {
        let card = self
            .session
            .cards
            .iter()
            .find(|card| card.id == card_id)
            .ok_or_else(|| format!("Unknown card {card_id}"))?;
        if !card.source.symbol.range.contains(position) {
            return Err("Requested source position is outside the card".into());
        }
        self.language.hover(&card.source.symbol.path, position)
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

    /// Toggle a picker symbol without changing the idempotent add/expand APIs.
    pub fn toggle_symbol(&mut self, symbol: Symbol, position: Point) -> Result<Option<String>> {
        if let Some(id) = self.card_for_symbol(&symbol).map(|card| card.id.clone()) {
            self.remove_card(&id)?;
            Ok(None)
        } else {
            self.add_symbol(symbol, position).map(Some)
        }
    }

    pub fn toggle_definition(
        &mut self,
        card_id: &str,
        position: Position,
    ) -> Result<Option<Vec<String>>> {
        self.toggle_expansion(card_id, position, ConnectionKind::Definition)
    }

    pub fn inspect_variable(
        &mut self,
        card_id: &str,
        position: Position,
    ) -> Result<Option<VariableInspection>> {
        let card = self
            .session
            .cards
            .iter()
            .find(|card| card.id == card_id)
            .ok_or_else(|| format!("Unknown card {card_id}"))?;
        if !card.source.symbol.range.contains(position) {
            return Err("Requested source position is outside the card".into());
        }
        let Some(token) = card.source.variable_token(position) else {
            return Ok(None);
        };
        let position = Position::new(token.line, token.start);
        let selected = SourceRange {
            start: position,
            end: Position::new(token.line, token.start + token.length),
        };
        let path = card.source.symbol.path.clone();
        let mut highlights = self.language.document_highlights(&path, position)?;
        for range in &highlights {
            range.validate()?;
        }
        if !highlights.contains(&selected) {
            highlights.push(selected);
        }
        let description = self.language.hover(&path, position)?;
        Ok(Some(VariableInspection {
            path,
            position,
            highlights,
            description,
        }))
    }

    pub fn toggle_type_definition(
        &mut self,
        card_id: &str,
        position: Position,
    ) -> Result<Option<Vec<String>>> {
        self.toggle_expansion(card_id, position, ConnectionKind::TypeDefinition)
    }

    pub fn toggle_references(
        &mut self,
        card_id: &str,
        position: Position,
    ) -> Result<Option<Vec<String>>> {
        self.toggle_expansion(card_id, position, ConnectionKind::Reference)
    }

    fn toggle_expansion(
        &mut self,
        card_id: &str,
        position: Position,
        kind: ConnectionKind,
    ) -> Result<Option<Vec<String>>> {
        let mut targets: Vec<_> = self
            .session
            .connections
            .iter()
            .filter(|edge| edge.from == card_id && edge.source == position && edge.kind == kind)
            .map(|edge| edge.to.clone())
            .collect();
        if targets.is_empty() {
            return self.expand(card_id, position, kind).map(Some);
        }
        targets.sort();
        targets.dedup();
        // Hide a group in one layout pass, while keeping the clicked source card.
        targets.retain(|target| target != card_id);
        self.remove_cards(&targets, &[card_id.into()])?;
        self.session
            .connections
            .retain(|edge| !(edge.from == card_id && edge.source == position && edge.kind == kind));
        Ok(None)
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
            ConnectionKind::TypeDefinition => self
                .language
                .type_definitions(&origin.source.symbol.path, position)?,
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
        let mut next_position = Point::new(
            origin.position.x + origin.width + CARD_COLUMN_GAP,
            source_anchor_y(&origin, position),
        );
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
        self.remove_cards(&[id.into()], &[])
    }

    fn remove_cards(&mut self, ids: &[String], preserved: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        for id in ids {
            if !self.session.cards.iter().any(|card| &card.id == id) {
                return Err(format!("Unknown card {id}"));
            }
        }
        let removed = descendant_cards(&self.session.connections, ids, preserved);
        let anchor = Point::new(
            self.session
                .cards
                .iter()
                .map(|card| card.position.x)
                .fold(f32::INFINITY, f32::min),
            self.session
                .cards
                .iter()
                .map(|card| card.position.y)
                .fold(f32::INFINITY, f32::min),
        );
        let mut cards = self.session.cards.clone();
        cards.retain(|card| !removed.contains(&card.id));
        compact_cards(&mut cards, anchor, &self.session.connections)?;
        self.session.cards = cards;
        self.session
            .connections
            .retain(|edge| !removed.contains(&edge.from) && !removed.contains(&edge.to));
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

// Include the visible file frame and its title in the space between rows.
const CARD_GAP: f32 = CODE_REGION_HEADER + CODE_REGION_PADDING + 16.0;
const CARD_COLUMN_GAP: f32 = 100.0;
const COLUMN_ALIGNMENT_TOLERANCE: f32 = 32.0;

fn source_anchor_y(card: &CodeCard, position: Position) -> f32 {
    card.position.y
        + CODE_CARD_HEADER
        + 8.0
        + position
            .line
            .saturating_sub(card.source.symbol.range.start.line) as f32
            * CODE_LINE_HEIGHT
}

/// Close the branch, preserving the clicked source and descendants reached by another branch.
/// Work from the original graph so cycles cannot strand an orphaned group.
fn descendant_cards(
    connections: &[Connection],
    ids: &[String],
    preserved: &[String],
) -> BTreeSet<String> {
    let explicit: BTreeSet<_> = ids.iter().cloned().collect();
    let mut candidates = explicit.clone();
    let mut pending = ids.to_vec();
    while let Some(id) = pending.pop() {
        for edge in connections.iter().filter(|edge| edge.from == id) {
            if !preserved.contains(&edge.to) && candidates.insert(edge.to.clone()) {
                pending.push(edge.to.clone());
            }
        }
    }
    let mut retained = BTreeSet::new();
    let mut pending: Vec<_> = connections
        .iter()
        .filter(|edge| {
            !candidates.contains(&edge.from)
                && candidates.contains(&edge.to)
                && !explicit.contains(&edge.to)
        })
        .map(|edge| edge.to.clone())
        .collect();
    while let Some(id) = pending.pop() {
        if !retained.insert(id.clone()) {
            continue;
        }
        for edge in connections.iter().filter(|edge| edge.from == id) {
            if candidates.contains(&edge.to) && !explicit.contains(&edge.to) {
                pending.push(edge.to.clone());
            }
        }
    }
    candidates.retain(|id| !retained.contains(id));
    candidates
}

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

/// Pack the remaining columns from the previous canvas origin after cards close.
/// Keep their horizontal column order and vertical reading order, using full source sizes.
fn compact_cards(cards: &mut [CodeCard], anchor: Point, connections: &[Connection]) -> Result<()> {
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
    fn toggling_visible_symbols_cleans_links_and_reopens_the_card() {
        let mut explorer = explorer();
        let origin = explorer
            .add_symbol(symbol("origin"), Point::default())
            .unwrap();
        let target = explorer
            .toggle_definition(&origin, Position::new(0, 4))
            .unwrap()
            .unwrap()
            .remove(0);
        assert_eq!(explorer.session.cards.len(), 2);
        assert!(
            explorer
                .toggle_definition(&origin, Position::new(0, 4))
                .unwrap()
                .is_none()
        );
        assert_eq!(explorer.session.cards.len(), 1);
        assert!(explorer.session.connections.is_empty());
        assert_eq!(
            explorer
                .toggle_definition(&origin, Position::new(0, 4))
                .unwrap()
                .unwrap(),
            vec![target.clone()]
        );
        // A search symbol may carry a different opaque ID for the same source range.
        let mut selected = symbol("target");
        selected.id = "search-result-id".into();
        assert!(
            explorer
                .toggle_symbol(selected.clone(), Point::default())
                .unwrap()
                .is_none()
        );
        assert!(explorer.session.connections.is_empty());
        assert!(
            !explorer
                .session
                .regions
                .iter()
                .any(|region| region.card_ids.contains(&target))
        );
        assert!(
            explorer
                .toggle_symbol(selected, Point::default())
                .unwrap()
                .is_some()
        );
        assert_eq!(explorer.session.cards.len(), 2);
        explorer.session.validate().unwrap();
    }

    #[test]
    fn reference_toggle_keeps_source_and_definition_toggle_hides_every_target() {
        let mut explorer = explorer();
        let origin = explorer
            .add_symbol(symbol("origin"), Point::default())
            .unwrap();
        explorer.language.additional.push(symbol("second_target"));
        explorer
            .toggle_definition(&origin, Position::new(0, 4))
            .unwrap();
        explorer.language.target = symbol("origin");
        explorer
            .toggle_references(&origin, Position::new(0, 4))
            .unwrap();
        assert!(
            explorer
                .toggle_references(&origin, Position::new(0, 4))
                .unwrap()
                .is_none()
        );
        assert_eq!(explorer.session.cards.len(), 3);
        assert!(
            explorer
                .toggle_definition(&origin, Position::new(0, 4))
                .unwrap()
                .is_none()
        );
        assert_eq!(explorer.session.cards.len(), 1);
        assert_eq!(explorer.session.cards[0].id, origin);
        assert!(explorer.session.connections.is_empty());
        explorer.session.validate().unwrap();
    }

    #[test]
    fn closing_a_child_removes_its_descendants_for_every_close_action() {
        for action in 0..3 {
            let mut explorer = explorer();
            let root = explorer
                .add_symbol(symbol("root"), Point::default())
                .unwrap();
            explorer.language.target = symbol("child");
            let child = explorer
                .expand_definition(&root, Position::new(0, 4))
                .unwrap()
                .remove(0);
            explorer.language.target = symbol("grandchild");
            let grandchild = explorer
                .expand_references(&child, Position::new(0, 4))
                .unwrap()
                .remove(0);
            explorer.language.target = symbol("great_grandchild");
            explorer
                .expand_definition(&grandchild, Position::new(0, 4))
                .unwrap();
            match action {
                0 => explorer.remove_card(&child).unwrap(),
                1 => {
                    explorer
                        .toggle_symbol(symbol("child"), Point::default())
                        .unwrap();
                }
                _ => {
                    explorer
                        .toggle_definition(&root, Position::new(0, 4))
                        .unwrap();
                }
            }
            assert_eq!(explorer.session.cards.len(), 1);
            assert_eq!(explorer.session.cards[0].id, root);
            assert!(explorer.session.connections.is_empty());
            explorer.session.validate().unwrap();
        }
    }

    #[test]
    fn closing_a_branch_preserves_shared_descendants_and_handles_cycles() {
        let mut explorer = explorer();
        let root = explorer
            .add_symbol(symbol("root"), Point::default())
            .unwrap();
        explorer.language.target = symbol("child");
        let child = explorer
            .expand_definition(&root, Position::new(0, 4))
            .unwrap()
            .remove(0);
        explorer.language.target = symbol("grandchild");
        let grandchild = explorer
            .expand_definition(&child, Position::new(0, 4))
            .unwrap()
            .remove(0);
        explorer.language.target = symbol("child");
        explorer
            .expand_definition(&grandchild, Position::new(0, 4))
            .unwrap();
        let other = explorer
            .add_symbol(symbol("other"), Point::new(0.0, 500.0))
            .unwrap();
        explorer.language.target = symbol("grandchild");
        explorer
            .expand_definition(&other, Position::new(0, 4))
            .unwrap();
        explorer.remove_card(&child).unwrap();
        assert_eq!(explorer.session.cards.len(), 3);
        assert!(
            explorer
                .session
                .cards
                .iter()
                .any(|card| card.id == grandchild)
        );
        explorer.remove_card(&other).unwrap();
        assert_eq!(explorer.session.cards.len(), 1);
        assert_eq!(explorer.session.cards[0].id, root);
        explorer.session.validate().unwrap();
    }

    #[test]
    fn toggling_a_cyclic_branch_preserves_the_clicked_source() {
        let mut explorer = explorer();
        let root = explorer
            .add_symbol(symbol("root"), Point::default())
            .unwrap();
        explorer.language.target = symbol("child");
        let child = explorer
            .expand_definition(&root, Position::new(0, 4))
            .unwrap()
            .remove(0);
        explorer.language.target = symbol("root");
        explorer
            .expand_definition(&child, Position::new(0, 4))
            .unwrap();
        explorer
            .toggle_definition(&root, Position::new(0, 4))
            .unwrap();
        assert_eq!(explorer.session.cards.len(), 1);
        assert_eq!(explorer.session.cards[0].id, root);
        assert!(explorer.session.connections.is_empty());
        explorer.session.validate().unwrap();
    }

    #[test]
    fn expansion_and_compaction_anchor_children_to_the_absolute_source_row() {
        for references in [false, true] {
            let mut explorer = explorer();
            explorer.language.code = std::iter::repeat_n("    call();", 100)
                .collect::<Vec<_>>()
                .join("\n");
            let mut source = symbol("long_parent");
            source.range = SourceRange {
                start: Position::new(20, 5),
                end: Position::new(120, 0),
            };
            source.selection_range = SourceRange {
                start: Position::new(20, 5),
                end: Position::new(20, 9),
            };
            let root = explorer
                .add_symbol(source, Point::new(100.0, 80.0))
                .unwrap();
            let unrelated = explorer
                .add_symbol(symbol("unrelated"), Point::new(0.0, 3000.0))
                .unwrap();
            explorer.language.code = "fn target() {}".into();
            let position = Position::new(70, 4);
            let child = if references {
                explorer.expand_references(&root, position)
            } else {
                explorer.expand_definition(&root, position)
            }
            .unwrap()
            .remove(0);
            let expected_offset = CODE_CARD_HEADER + 8.0 + 50.0 * CODE_LINE_HEIGHT;
            let assert_anchor = |explorer: &Explorer<Language, Repository>| {
                let parent = explorer
                    .session
                    .cards
                    .iter()
                    .find(|card| card.id == root)
                    .unwrap();
                let target = explorer
                    .session
                    .cards
                    .iter()
                    .find(|card| card.id == child)
                    .unwrap();
                assert_eq!(target.position.y, parent.position.y + expected_offset);
                assert_eq!(
                    target.position.x,
                    parent.position.x + parent.width + CARD_COLUMN_GAP
                );
                assert!(!CardRect::from(parent).overlaps(CardRect::from(target)));
            };
            assert_anchor(&explorer);
            explorer.remove_card(&unrelated).unwrap();
            assert_anchor(&explorer);
            explorer.session.validate().unwrap();
        }
    }

    #[test]
    fn stacked_cards_leave_room_for_file_region_headers_and_padding() {
        let mut explorer = explorer();
        explorer
            .add_symbol(symbol("first"), Point::default())
            .unwrap();
        explorer
            .add_symbol(symbol("second"), Point::default())
            .unwrap();
        let cards = &explorer.session.cards;
        // The file frame extends 22 below each card and 36 above the next.
        assert!(
            cards[0].position.y + cards[0].display_height() + 22.0 < cards[1].position.y - 36.0
        );
        arrange_cards(&mut explorer.session.cards).unwrap();
        let cards = &explorer.session.cards;
        assert!(
            cards[0].position.y + cards[0].display_height() + 22.0 < cards[1].position.y - 36.0
        );
    }

    #[test]
    fn closing_cards_preserves_columns_when_a_lower_card_is_wider() {
        let mut explorer = explorer();
        let root = explorer
            .add_symbol(symbol("root"), Point::default())
            .unwrap();
        let child = explorer
            .expand_definition(&root, Position::new(0, 4))
            .unwrap()
            .remove(0);
        explorer.language.code = "x".repeat(150);
        let wide = explorer
            .add_symbol(symbol("wide"), Point::new(0.0, 1000.0))
            .unwrap();
        let unrelated = explorer
            .add_symbol(symbol("unrelated"), Point::new(2000.0, 2000.0))
            .unwrap();
        let viewport = explorer.session.viewport;
        explorer.remove_card(&unrelated).unwrap();
        let root = explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == root)
            .unwrap();
        let child = explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == child)
            .unwrap();
        let wide = explorer
            .session
            .cards
            .iter()
            .find(|card| card.id == wide)
            .unwrap();
        assert_eq!(root.position.x, wide.position.x);
        assert!(child.position.x >= wide.position.x + wide.width + CARD_COLUMN_GAP);
        assert_eq!(child.position.y, source_anchor_y(root, Position::new(0, 4)));
        assert_eq!(explorer.session.viewport, viewport);
        for (index, card) in explorer.session.cards.iter().enumerate() {
            for other in &explorer.session.cards[index + 1..] {
                assert!(!CardRect::from(card).overlaps(CardRect::from(other)));
            }
        }
    }

    #[test]
    fn hiding_cards_packs_rows_and_columns_without_changing_the_viewport() {
        let mut explorer = explorer();
        explorer.language.code = std::iter::repeat_n("fn source() {}", 20)
            .collect::<Vec<_>>()
            .join("\n");
        let height = CodeCard::source_height(&SourceDocument {
            symbol: symbol("root"),
            code: explorer.language.code.clone(),
            tokens: vec![],
        });
        let root = explorer
            .add_symbol(symbol("root"), Point::new(100.0, 80.0))
            .unwrap();
        let middle = explorer
            .add_symbol(
                symbol("middle"),
                Point::new(100.0, 80.0 + height + CARD_GAP),
            )
            .unwrap();
        let bottom = explorer
            .add_symbol(
                symbol("bottom"),
                Point::new(100.0, 80.0 + 2.0 * (height + CARD_GAP)),
            )
            .unwrap();
        let column = explorer
            .add_symbol(symbol("column"), Point::new(720.0, 80.0))
            .unwrap();
        let far = explorer
            .add_symbol(symbol("far"), Point::new(1340.0, 80.0))
            .unwrap();
        explorer.pan(Point::new(-150.0, 65.0)).unwrap();
        explorer.zoom(0.75, Point::new(200.0, 180.0)).unwrap();
        let viewport = explorer.session.viewport;
        explorer
            .toggle_symbol(symbol("middle"), Point::default())
            .unwrap();
        assert!(!explorer.session.cards.iter().any(|card| card.id == middle));
        assert_eq!(
            explorer
                .session
                .cards
                .iter()
                .find(|card| card.id == bottom)
                .unwrap()
                .position,
            Point::new(100.0, 80.0 + height + CARD_GAP)
        );
        explorer.remove_card(&column).unwrap();
        assert_eq!(
            explorer
                .session
                .cards
                .iter()
                .find(|card| card.id == far)
                .unwrap()
                .position,
            Point::new(720.0, 80.0)
        );
        assert_eq!(
            explorer
                .session
                .cards
                .iter()
                .find(|card| card.id == root)
                .unwrap()
                .position,
            Point::new(100.0, 80.0)
        );
        assert_eq!(explorer.session.viewport, viewport);
        for (index, card) in explorer.session.cards.iter().enumerate() {
            for other in &explorer.session.cards[index + 1..] {
                assert!(!CardRect::from(card).overlaps(CardRect::from(other)));
            }
        }
        let before = explorer.session.clone();
        assert!(explorer.remove_card("missing").is_err());
        assert_eq!(explorer.session, before);
        explorer.session.validate().unwrap();
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
        assert_eq!(cards[1].position.y, 876.0 + CARD_GAP);
        assert_eq!(cards[2].position.y, 2.0 * (876.0 + CARD_GAP));
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
