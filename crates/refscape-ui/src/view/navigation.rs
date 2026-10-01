//! Source navigation and application commands.
use super::*;

impl<L: LanguageService + 'static, R: SessionRepository + 'static> ExplorerView<L, R> {
    pub(super) fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.query.clone();
        self.run_job(
            "Searching workspace symbols…",
            Box::new(move |explorer| {
                Ok(Output {
                    symbols: Some(explorer.search(&query)?),
                    ..Default::default()
                })
            }),
            cx,
        );
    }

    pub(super) fn insertion_point(&self) -> Point {
        self.session
            .viewport
            .screen_to_world(Point::new(100.0, 80.0))
    }
    pub(super) fn add_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let position = self.insertion_point();
        self.run_job(
            "Opening source file…",
            Box::new(move |explorer| {
                Ok(Output {
                    prepared: Some(explorer.prepare_add_file(&path, position)?),
                    symbols: Some(explorer.symbols(&path)?),
                    ..Default::default()
                })
            }),
            cx,
        );
    }
    pub(super) fn toggle_symbol(&mut self, symbol: Symbol, cx: &mut Context<Self>) {
        if let Some(id) = self
            .session
            .cards
            .iter()
            .find(|card| {
                card.source.symbol.path == symbol.path
                    && (card.source.symbol.id == symbol.id
                        || (card.source.symbol.kind == symbol.kind
                            && card.source.symbol.range == symbol.range))
            })
            .map(|card| card.id.clone())
        {
            self.layout_activity(cx);
            self.canvas.selected = Some(id);
            cx.notify();
            self.remove_selected(cx);
            return;
        }
        let position = self.insertion_point();
        self.run_job(
            "Toggling symbol…",
            Box::new(move |explorer| {
                Ok(Output {
                    prepared: Some(explorer.prepare_add_symbol(symbol, position)?),
                    ..Default::default()
                })
            }),
            cx,
        );
    }
    pub(super) fn remove_selected(&mut self, cx: &mut Context<Self>) {
        if self.requests.busy || self.requests.closing {
            return;
        }
        if let Some(id) = self.canvas.selected.take() {
            self.clear_inspection();
            self.run_job(
                "Removing card…",
                Box::new(move |explorer| {
                    explorer.remove_card(&id)?;
                    Ok(Output::default())
                }),
                cx,
            );
        }
    }
    pub(super) fn cycle_theme(&mut self, cx: &mut Context<Self>) {
        let index = self
            .project
            .themes
            .iter()
            .position(|t| *t == self.session.theme)
            .unwrap_or(0);
        let theme = self.project.themes[(index + 1) % self.project.themes.len()].clone();
        self.run_job(
            "Applying theme…",
            Box::new(move |explorer| {
                explorer.set_theme(theme)?;
                Ok(Output::default())
            }),
            cx,
        );
    }
}
