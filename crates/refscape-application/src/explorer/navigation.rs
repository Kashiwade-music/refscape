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
        if !card.source.contains_display_position(position) {
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
        if !card.source.contains_display_position(position) {
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

    /// Compatibility anchor for nonvisual callers; UI supplies measured word geometry.
    fn estimated_anchor(&self, card_id: &str, position: Position) -> Result<Point> {
        let card = self
            .session
            .cards
            .iter()
            .find(|card| card.id == card_id)
            .ok_or_else(|| format!("Unknown card {card_id}"))?;
        Ok(Point::new(
            card.width,
            source_anchor_y(card, position) - card.position.y,
        ))
    }

    fn toggle_expansion(
        &mut self,
        card_id: &str,
        position: Position,
        kind: ConnectionKind,
    ) -> Result<Option<Vec<String>>> {
        let anchor = self.estimated_anchor(card_id, position)?;
        let edit = self.prepare_toggle_expansion(card_id, position, kind, anchor)?;
        let outcome = self.commit_prepared(edit)?;
        Ok(if outcome.expanded == Some(true) {
            Some(outcome.targets)
        } else {
            None
        })
    }

    pub fn expand_definition(&mut self, card_id: &str, position: Position) -> Result<Vec<String>> {
        self.expand(card_id, position, ConnectionKind::Definition)
    }

    pub fn expand_references(&mut self, card_id: &str, position: Position) -> Result<Vec<String>> {
        self.expand(card_id, position, ConnectionKind::Reference)
    }

    /// Explicit world offset route for CLI/tests that know the symbol anchor.
    pub fn expand_at(
        &mut self,
        card_id: &str,
        position: Position,
        kind: ConnectionKind,
        anchor_offset: Point,
    ) -> Result<Vec<String>> {
        let edit = self.prepare_expansion(card_id, position, kind, anchor_offset)?;
        Ok(self.commit_prepared(edit)?.targets)
    }

    fn expand(
        &mut self,
        card_id: &str,
        position: Position,
        kind: ConnectionKind,
    ) -> Result<Vec<String>> {
        let anchor = self.estimated_anchor(card_id, position)?;
        self.expand_at(card_id, position, kind, anchor)
    }
}
