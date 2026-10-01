use super::*;

impl<L: LanguageService, R: SessionRepository> Explorer<L, R> {
    /// Reveal one contiguous omitted span in this card, retaining its source coordinates.
    pub fn expand_context(&mut self, id: &str, index: usize) -> Result<()> {
        let card_index = self
            .session
            .cards
            .iter()
            .position(|card| card.id == id)
            .ok_or_else(|| format!("Unknown card {id}"))?;
        let mut source = self.session.cards[card_index].source.clone();
        let range = source
            .folded_range(index)
            .ok_or("This row has no hidden source")?;
        let hidden = if let Some(hidden) = source
            .folded
            .iter()
            .find(|gap| gap.start_line == range.start)
        {
            hidden.code.clone()
        } else {
            // Older saved cards have declaration excerpts but no snapshot of the gaps.
            let file = self.language.source(&Symbol::file(
                source.symbol.path.clone(),
                SourceRange::default(),
            ))?;
            file.validate()?;
            let start = file.code_start.unwrap_or(file.symbol.range.start).line;
            let offset = range
                .start
                .checked_sub(start)
                .ok_or("Hidden source is outside the document")?;
            let lines: Vec<_> = file
                .code
                .lines()
                .skip(offset as usize)
                .take((range.end - range.start) as usize)
                .collect();
            if lines.len() != (range.end - range.start) as usize {
                return Err("Hidden source is outside the document".into());
            }
            for token in file
                .tokens
                .into_iter()
                .filter(|token| range.contains(&token.line))
            {
                if !source.tokens.contains(&token) {
                    source.tokens.push(token);
                }
            }
            format!("{}\n", lines.join("\n"))
        };
        let context = &mut source.context[index];
        if !context.code.ends_with('\n') {
            context.code.push('\n');
        }
        context.code.push_str(&hidden);
        source.expanded.push(refscape_model::SourceContext {
            start_line: range.start,
            code: hidden,
        });
        source.folded.retain(|gap| gap.start_line != range.start);
        self.replace_card_source(card_index, source)
    }

    pub fn collapse_context(&mut self, id: &str, index: usize) -> Result<()> {
        let card_index = self
            .session
            .cards
            .iter()
            .position(|card| card.id == id)
            .ok_or_else(|| format!("Unknown card {id}"))?;
        let mut source = self.session.cards[card_index].source.clone();
        let hidden = source
            .expanded_context(index)
            .cloned()
            .ok_or("This section is not expanded")?;
        let context = &mut source.context[index];
        context.code = context
            .code
            .lines()
            .take((hidden.start_line - context.start_line) as usize)
            .collect::<Vec<_>>()
            .join("\n");
        source
            .expanded
            .retain(|gap| gap.start_line != hidden.start_line);
        source.folded.push(hidden);
        source.folded.sort_by_key(|gap| gap.start_line);
        self.replace_card_source(card_index, source)
    }

    fn replace_card_source(&mut self, card_index: usize, source: SourceDocument) -> Result<()> {
        source.validate()?;
        let mut cards = self.session.cards.clone();
        let (width, height) = source_dimensions(&source);
        cards[card_index].source = source;
        cards[card_index].width = width;
        cards[card_index].height = height;
        let anchor = Point::new(
            cards
                .iter()
                .map(|card| card.position.x)
                .fold(f32::INFINITY, f32::min),
            cards
                .iter()
                .map(|card| card.position.y)
                .fold(f32::INFINITY, f32::min),
        );
        compact_cards(&mut cards, anchor, &self.session.connections)?;
        self.session.cards = cards;
        self.rebuild_regions();
        Ok(())
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
