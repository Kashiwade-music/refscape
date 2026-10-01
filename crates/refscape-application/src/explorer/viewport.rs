use super::*;

impl<L: LanguageService, R: SessionRepository> Explorer<L, R> {
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
        validate_layout(&cards, LayoutRules::default())?;
        let mut candidate = self.session.clone();
        candidate.cards = cards.clone();
        candidate.viewport = viewport;
        // The project picker starts with an intentionally empty, unopened session.
        // Camera synchronization there must not require a project root yet.
        if !candidate.project_root.as_os_str().is_empty()
            || !candidate.cards.is_empty()
            || !candidate.connections.is_empty()
        {
            candidate.validate()?;
        }
        let changed = cards != self.session.cards;
        self.session.viewport = viewport;
        self.session.cards = cards;
        self.rebuild_regions();
        if changed {
            self.geometry_changed();
        }
        Ok(())
    }

    pub fn set_theme(&mut self, theme: Theme) -> Result<()> {
        theme.validate()?;
        self.session.theme = theme;
        Ok(())
    }
}
