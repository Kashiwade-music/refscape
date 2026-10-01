use super::*;

impl<L: LanguageService, R: SessionRepository> Explorer<L, R> {
    pub fn expand_context(&mut self, id: &str, index: usize) -> Result<()> {
        let edit = self.prepare_context(id, index, true)?;
        self.commit_prepared(edit).map(|_| ())
    }

    pub fn collapse_context(&mut self, id: &str, index: usize) -> Result<()> {
        let edit = self.prepare_context(id, index, false)?;
        self.commit_prepared(edit).map(|_| ())
    }

    pub(super) fn context_source(
        &mut self,
        id: &str,
        index: usize,
        expand: bool,
    ) -> Result<SourceDocument> {
        if expand {
            self.expanded_source(id, index)
        } else {
            self.collapsed_source(id, index)
        }
    }
    /// Reveal one contiguous omitted span in this card, retaining its source coordinates.
    fn expanded_source(&mut self, id: &str, index: usize) -> Result<SourceDocument> {
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
        Ok(source)
    }

    fn collapsed_source(&mut self, id: &str, index: usize) -> Result<SourceDocument> {
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
        Ok(source)
    }

    pub(super) fn replace_card_source(
        &mut self,
        card_index: usize,
        source: SourceDocument,
    ) -> Result<()> {
        source.validate()?;
        let mut cards = self.session.cards.clone();
        let (width, height) = source_dimensions(&source);
        let plan = plan_resize(
            &cards,
            &cards[card_index].id,
            width,
            height,
            LayoutRules::default(),
        )?;
        cards[card_index].source = source;
        cards[card_index].width = width;
        cards[card_index].height = height;
        plan.apply_positions(&mut cards, LayoutRules::default())?;
        let mut session = self.session.clone();
        session.cards = cards.clone();
        session.validate()?;
        self.session.cards = cards;
        self.rebuild_regions();
        self.content_changed();
        Ok(())
    }

    pub fn add_symbol(&mut self, symbol: Symbol, position: Point) -> Result<String> {
        let edit = self.prepare_add_symbol(symbol, position)?;
        self.commit_prepared(edit)?
            .targets
            .into_iter()
            .next()
            .ok_or("Added source has no card".into())
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
        let target = cards
            .iter()
            .find(|card| card.id == id)
            .ok_or_else(|| format!("Unknown card {id}"))?;
        let occupied: Vec<_> = cards
            .iter()
            .filter(|card| card.id != id)
            .map(CardRect::from)
            .collect();
        let placement = nearest_vacant_position(
            position,
            target.width,
            target.display_height(),
            None,
            &occupied,
            LayoutRules::default(),
        )?;
        cards
            .iter_mut()
            .find(|card| card.id == id)
            .unwrap()
            .position = placement.position;
        validate_layout(&cards, LayoutRules::default())?;
        let mut candidate = self.session.clone();
        candidate.cards = cards.clone();
        candidate.validate()?;
        let changed = cards != self.session.cards;
        self.session.cards = cards;
        self.rebuild_regions();
        if changed {
            self.geometry_changed();
        }
        self.layout_undo = None;
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
        let mut cards = self.session.cards.clone();
        cards.retain(|card| !removed.contains(&card.id));
        validate_layout(&cards, LayoutRules::default())?;
        self.session.cards = cards;
        self.session
            .connections
            .retain(|edge| !removed.contains(&edge.from) && !removed.contains(&edge.to));
        self.rebuild_regions();
        self.content_changed();
        Ok(())
    }
}
