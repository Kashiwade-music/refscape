use super::*;

impl<L: LanguageService, R: SessionRepository> Explorer<L, R> {
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
}
