//! Pointer and keyboard input, selection, and canvas movement.
use super::*;

pub(super) enum Drag {
    Pan(gpui::Point<Pixels>),
    Card(String, gpui::Point<Pixels>, Point),
}

impl<L: LanguageService + 'static, R: SessionRepository + 'static> ExplorerView<L, R> {
    pub(super) fn zoom(&mut self, factor: f32, anchor: Point, cx: &mut Context<Self>) {
        self.clear_hover(cx);
        self.layout_activity(cx);
        let viewport = &mut self.session.viewport;
        let old = viewport.zoom;
        let zoom = (old * factor).clamp(0.15, 3.0);
        viewport.offset = Point::new(
            anchor.x - (anchor.x - viewport.offset.x) * zoom / old,
            anchor.y - (anchor.y - viewport.offset.y) * zoom / old,
        );
        viewport.zoom = zoom;
        cx.notify();
    }
    pub(super) fn fit(&mut self, cx: &mut Context<Self>) {
        self.clear_hover(cx);
        self.layout_activity(cx);
        if self.session.cards.is_empty() {
            self.session.viewport = Default::default();
            cx.notify();
            return;
        }
        let left = self
            .session
            .cards
            .iter()
            .map(|c| c.position.x)
            .fold(f32::INFINITY, f32::min);
        let top = self
            .session
            .cards
            .iter()
            .map(|c| c.position.y)
            .fold(f32::INFINITY, f32::min);
        let right = self
            .session
            .cards
            .iter()
            .map(|c| c.position.x + c.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let bottom = self
            .session
            .cards
            .iter()
            .map(|c| c.position.y + card_height(c))
            .fold(f32::NEG_INFINITY, f32::max);
        let zoom = ((f32::from(self.canvas.bounds.size.width).max(600.0) - 80.0)
            / (right - left).max(1.0))
        .min(
            (f32::from(self.canvas.bounds.size.height).max(400.0) - 80.0) / (bottom - top).max(1.0),
        )
        .clamp(0.15, 2.0);
        self.session.viewport.zoom = zoom;
        self.session.viewport.offset = Point::new(40.0 - left * zoom, 40.0 - top * zoom);
        cx.notify();
    }

    pub(super) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        references: bool,
        cx: &mut Context<Self>,
    ) {
        if self.requests.closing {
            return;
        }
        self.clear_hover(cx);
        self.layout_activity(cx);
        self.search.focused = false;
        for card in self.canvas.painted.iter().rev() {
            if !card.bounds.contains(&event.position) {
                continue;
            }
            let id = card.id.clone();
            self.canvas.selected = Some(id.clone());
            let zoom = self.session.viewport.zoom;
            if f32::from(event.position.y - card.bounds.top()) < HEADER * zoom && !references {
                if self.layout.planning {
                    return;
                }
                if event.position.x > card.bounds.right() - px(28.0 * zoom) {
                    self.remove_selected(cx);
                } else if let Some(source) = self.session.cards.iter().find(|c| c.id == id) {
                    self.canvas.drag_preview = Some((id.clone(), source.position));
                    self.canvas.drag = Some(Drag::Card(id, event.position, source.position));
                }
                cx.notify();
                return;
            }
            if event.position.y < card.origin.y {
                cx.notify();
                return;
            }
            let line_index =
                (f32::from(event.position.y - card.origin.y) / (LINE * zoom)).floor() as usize;
            if !references
                && let Some(row) = card.rows.get(line_index)
                && let Some(index) = row.fold
                && (row.position.is_none()
                    || event.position.x < card.bounds.left() + px(24.0 * zoom))
            {
                let collapse = row.position.is_some();
                self.run_job(
                    if collapse {
                        "Hiding context…"
                    } else {
                        "Expanding context…"
                    },
                    Box::new(move |explorer| {
                        Ok(Output {
                            prepared: Some(explorer.prepare_context(&id, index, !collapse)?),
                            ..Default::default()
                        })
                    }),
                    cx,
                );
                return;
            }
            if let Some(row) = card.rows.get(line_index)
                && let Some(source_position) = row.position
                && event.position.x >= card.origin.x
                && let Some(byte) = row.code.index_for_x(event.position.x - card.origin.x)
            {
                let line = &row.code;
                let position = Position::new(
                    source_position.line,
                    line.text[..byte].encode_utf16().count() as u32 + source_position.character,
                );
                if self.requests.busy {
                    return;
                }
                let direct_definition = event.modifiers.alt && !references;
                let kind = if references {
                    ConnectionKind::Reference
                } else if !direct_definition
                    && self
                        .session
                        .cards
                        .iter()
                        .find(|card| card.id == id)
                        .is_some_and(|card| card.source.variable_token(position).is_some())
                {
                    ConnectionKind::TypeDefinition
                } else {
                    ConnectionKind::Definition
                };
                // Reuse the link's position when another glyph of the same word
                // is clicked, including saved links created before toggling existed.
                let position = self
                    .session
                    .cards
                    .iter()
                    .find(|card| card.id == id)
                    .and_then(|card| {
                        let word = connected_word(card, position)?;
                        self.session
                            .connections
                            .iter()
                            .find(|edge| {
                                edge.from == id
                                    && edge.kind == kind
                                    && connected_word(card, edge.source).as_ref() == Some(&word)
                            })
                            .map(|edge| edge.source)
                    })
                    .unwrap_or(position);
                let Some(anchor) = self
                    .session
                    .cards
                    .iter()
                    .find(|source| source.id == id)
                    .and_then(|source| shaping::symbol_anchor_offset(source, card, position))
                else {
                    return;
                };
                self.clear_inspection();
                let generation = self.canvas.selection_generation;
                self.run_job(
                    if references {
                        "Finding references…"
                    } else if kind == ConnectionKind::TypeDefinition {
                        "Finding variable uses and type…"
                    } else {
                        "Finding definitions…"
                    },
                    Box::new(move |explorer| {
                        let inspection = if kind == ConnectionKind::TypeDefinition {
                            explorer.inspect_variable(&id, position)?
                        } else {
                            None
                        };
                        let prepared = explorer.prepare_toggle_expansion(
                            &id,
                            inspection.as_ref().map_or(position, |value| value.position),
                            kind,
                            anchor,
                        );
                        let prepared = match prepared {
                            Ok(prepared) => prepared,
                            Err(error) if inspection.is_some() => {
                                return Ok(Output {
                                    inspection: inspection
                                        .map(|inspection| (generation, inspection)),
                                    message: Some(format!(
                                        "Variable highlighted; cannot expand its type: {error}"
                                    )),
                                    error: true,
                                    ..Default::default()
                                });
                            }
                            Err(error) => return Err(error),
                        };
                        Ok(Output {
                            prepared: Some(prepared),
                            inspection: inspection.map(|inspection| (generation, inspection)),
                            ..Default::default()
                        })
                    }),
                    cx,
                );
            }
            cx.notify();
            return;
        }
        self.canvas.selected = None;
        self.clear_inspection();
        if !references {
            self.canvas.drag = Some(Drag::Pan(event.position));
        }
        cx.notify();
    }

    pub(super) fn clear_inspection(&mut self) {
        self.canvas.inspection = None;
        self.canvas.selection_generation = self.canvas.selection_generation.wrapping_add(1);
    }

    pub(super) fn context_control_at(&self, mouse: gpui::Point<Pixels>) -> Option<(String, usize)> {
        if self.session.viewport.zoom < 0.65 || !self.canvas.bounds.contains(&mouse) {
            return None;
        }
        let card = self
            .canvas
            .painted
            .iter()
            .rev()
            .find(|card| card.bounds.contains(&mouse))?;
        if mouse.y < card.origin.y {
            return None;
        }
        let row = (f32::from(mouse.y - card.origin.y) / (LINE * self.session.viewport.zoom)).floor()
            as usize;
        let row = card.rows.get(row)?;
        let index = row.fold?;
        if row.position.is_some()
            && mouse.x >= card.bounds.left() + px(24.0 * self.session.viewport.zoom)
        {
            return None;
        }
        Some((card.id.clone(), index))
    }
    pub(super) fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.requests.closing {
            return;
        }
        match &mut self.canvas.drag {
            Some(Drag::Pan(last)) => {
                self.session.viewport.offset.x += f32::from(event.position.x - last.x);
                self.session.viewport.offset.y += f32::from(event.position.y - last.y);
                *last = event.position;
            }
            Some(Drag::Card(id, start, origin)) => {
                if self.session.cards.iter().any(|c| c.id == *id) {
                    self.canvas.drag_preview = Some((
                        id.clone(),
                        Point::new(
                            origin.x
                                + f32::from(event.position.x - start.x)
                                    / self.session.viewport.zoom,
                            origin.y
                                + f32::from(event.position.y - start.y)
                                    / self.session.viewport.zoom,
                        ),
                    ));
                }
            }
            None => {
                self.update_hover(event.position, cx);
                return;
            }
        }
        self.layout_activity(cx);
        cx.notify();
    }

    pub(super) fn reconcile_canvas_selection(&mut self) {
        if self
            .canvas
            .selected
            .as_ref()
            .is_some_and(|id| !self.session.cards.iter().any(|card| &card.id == id))
        {
            self.canvas.selected = None;
        }
        if matches!(&self.canvas.drag, Some(Drag::Card(id, ..)) if !self.session.cards.iter().any(|card| &card.id == id))
        {
            self.canvas.drag = None;
            self.canvas.drag_preview = None;
        }
        if self
            .layout
            .pending_drop
            .as_ref()
            .is_some_and(|(id, _)| !self.session.cards.iter().any(|card| &card.id == id))
        {
            self.layout.pending_drop = None;
        }
    }

    pub(super) fn finish_drag(&mut self, cx: &mut Context<Self>) {
        if matches!(self.canvas.drag.take(), Some(Drag::Card(..))) {
            self.layout.pending_drop = self.canvas.drag_preview.take();
        }
        self.layout_activity(cx);
        self.resume_canvas_edit(cx);
        cx.notify();
    }
    pub(super) fn scroll(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.requests.closing {
            return;
        }
        self.clear_hover(cx);
        self.layout_activity(cx);
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(p) => (p.x * 24.0, p.y * 24.0),
        };
        if event.modifiers.control || event.modifiers.platform {
            self.zoom(
                (dy * 0.004).exp(),
                Point::new(
                    f32::from(event.position.x - self.canvas.bounds.left()),
                    f32::from(event.position.y - self.canvas.bounds.top()),
                ),
                cx,
            );
        } else {
            self.session.viewport.offset.x += if event.modifiers.shift { dy } else { dx };
            self.session.viewport.offset.y += if event.modifiers.shift { 0.0 } else { dy };
            cx.notify();
        }
    }

    pub(super) fn key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.requests.closing {
            return;
        }
        let key = event.keystroke.key.as_str();
        self.layout_activity(cx);
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform {
            match key {
                "s" if modifiers.shift => self.pick_session(true, cx),
                "s" => self.save(cx),
                "o" if modifiers.shift => self.pick_project(cx),
                "o" => self.pick_session(false, cx),
                "f" | "p" => {
                    self.search.focused = true;
                    self.search.selection = 0..self.search.query.len();
                    cx.notify();
                }
                "a" if self.search.focused => {
                    self.search.selection = 0..self.search.query.len();
                    cx.notify();
                }
                "v" if self.search.focused => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        self.replace_query(None, &text, cx);
                    }
                }
                _ => {}
            }
            cx.stop_propagation();
            return;
        }
        match key {
            "escape" => {
                self.clear_hover(cx);
                self.clear_inspection();
                self.search.focused = false;
                self.canvas.drag = None;
                self.canvas.drag_preview = None;
                self.resume_canvas_edit(cx);
                cx.notify();
            }
            "enter" if self.search.focused => self.search(cx),
            "backspace" if self.search.focused => {
                let range = if self.search.selection.is_empty() {
                    self.search.query[..self.search.selection.end]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0)..self.search.selection.end
                } else {
                    self.search.selection.clone()
                };
                self.replace_query(Some(range), "", cx);
            }
            "delete" if self.search.focused => {
                let start = self.search.selection.start;
                let end = if self.search.selection.is_empty() {
                    self.search.query[start..]
                        .chars()
                        .next()
                        .map(|ch| start + ch.len_utf8())
                        .unwrap_or(start)
                } else {
                    self.search.selection.end
                };
                self.replace_query(Some(start..end), "", cx);
            }
            "left" | "right" | "home" | "end" if self.search.focused => {
                let index = match key {
                    "home" => 0,
                    "end" => self.search.query.len(),
                    "left" => self.search.query[..self.search.selection.start]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0),
                    _ => self.search.query[self.search.selection.end..]
                        .chars()
                        .next()
                        .map(|ch| self.search.selection.end + ch.len_utf8())
                        .unwrap_or(self.search.query.len()),
                };
                self.search.selection = index..index;
                cx.notify();
            }
            "delete" | "backspace" => self.remove_selected(cx),
            "0" if !self.search.focused => self.fit(cx),
            "+" | "=" if !self.search.focused => self.zoom(1.15, Point::new(300.0, 200.0), cx),
            "-" if !self.search.focused => self.zoom(1.0 / 1.15, Point::new(300.0, 200.0), cx),
            _ => {}
        }
    }
}
