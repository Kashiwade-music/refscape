use super::*;

impl<L: LanguageService, R: SessionRepository> Explorer<L, R> {
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

    pub(super) fn remove_cards(&mut self, ids: &[String], preserved: &[String]) -> Result<()> {
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
}
