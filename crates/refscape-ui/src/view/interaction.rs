//! Pointer input is a gesture presenter; application commands own canvas mutations.
use super::{ExplorerView, HEADER, LINE, shaping};
use gpui::{
    Context, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, ScrollDelta,
    ScrollWheelEvent, Window, px,
};
use refscape_application::{Command, NavigationMode};
use refscape_model::Point;
pub(super) enum Drag {
    Pan(MouseButton, gpui::Point<Pixels>),
    Card(String, gpui::Point<Pixels>, Point),
}
impl ExplorerView {
    pub(super) fn zoom(&mut self, factor: f32, anchor: Point, cx: &mut Context<Self>) {
        self.clear_hover(cx);
        self.command(Command::Zoom { factor, anchor }, cx);
    }
    pub(super) fn fit(&mut self, cx: &mut Context<Self>) {
        self.clear_hover(cx);
        self.command(
            Command::Fit {
                width: f32::from(self.canvas.bounds.size.width).max(600.0),
                height: f32::from(self.canvas.bounds.size.height).max(400.0),
            },
            cx,
        );
    }
    pub(super) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        references: bool,
        cx: &mut Context<Self>,
    ) {
        if self.controller.closing() {
            return;
        }
        self.clear_hover(cx);
        self.search.focused = false;
        let snapshot = self.controller.shared_snapshot();
        for card in self.canvas.painted.iter().rev() {
            if !card.current(&snapshot) || !card.bounds.contains(&event.position) {
                continue;
            }
            let id = card.id.clone();
            self.canvas.selected = Some(id.clone());
            let zoom = snapshot.viewport.zoom;
            if f32::from(event.position.y - card.bounds.top()) < HEADER * zoom && !references {
                if event.position.x > card.bounds.right() - px(28.0 * zoom) {
                    self.remove_selected(cx);
                } else if let Some(source) = snapshot.cards.iter().find(|c| c.id.as_str() == id) {
                    self.canvas.drag_preview = Some((id.clone(), source.position.point()));
                    self.canvas.drag =
                        Some(Drag::Card(id, event.position, source.position.point()));
                }
                self.layout_activity(cx);
                cx.notify();
                return;
            }
            let index =
                (f32::from(event.position.y - card.origin.y) / (LINE * zoom)).floor() as usize;
            let Some(row) = card.row(index) else {
                return;
            };
            if !references
                && let Some(fold) = row.fold
                && (row.position.is_none()
                    || event.position.x < card.bounds.left() + px(24.0 * zoom))
            {
                self.command(
                    Command::ToggleFold {
                        card: id,
                        index: fold,
                        expand: row.position.is_none(),
                    },
                    cx,
                );
                return;
            }
            if event.position.x >= card.origin.x
                && let Some(position) = row.source_position(event.position.x - card.origin.x)
            {
                let Some(source) = snapshot
                    .cards
                    .iter()
                    .find(|source| source.id.as_str() == id)
                else {
                    return;
                };
                let Some(anchor) = shaping::symbol_anchor_offset(source, card, position) else {
                    return;
                };
                let mode = if references {
                    NavigationMode::References
                } else if event.modifiers.alt {
                    NavigationMode::Definition
                } else {
                    NavigationMode::Normal
                };
                self.clear_inspection();
                self.command(
                    Command::Click {
                        card: id,
                        position,
                        anchor,
                        mode,
                    },
                    cx,
                );
            }
            self.layout_activity(cx);
            cx.notify();
            return;
        }
        self.canvas.selected = None;
        self.clear_inspection();
        if !references {
            self.canvas.drag = Some(Drag::Pan(MouseButton::Left, event.position));
        }
        self.layout_activity(cx);
        cx.notify();
    }
    pub(super) fn clear_inspection(&mut self) {
        self.controller.dispatch(Command::CancelInspection);
        self.canvas.inspection = None;
        self.canvas.selection_generation = self.canvas.selection_generation.wrapping_add(1);
    }
    pub(super) fn context_control_at(&self, mouse: gpui::Point<Pixels>) -> Option<(String, usize)> {
        let zoom = self.controller.snapshot().viewport.zoom;
        if zoom < 0.65 || !self.canvas.bounds.contains(&mouse) {
            return None;
        }
        let card = self.canvas.painted.iter().rev().find(|card| {
            card.current(self.controller.snapshot()) && card.bounds.contains(&mouse)
        })?;
        if mouse.y < card.origin.y {
            return None;
        }
        let index = (f32::from(mouse.y - card.origin.y) / (LINE * zoom)).floor() as usize;
        let row = card.row(index)?;
        let fold = row.fold?;
        if row.position.is_some() && mouse.x >= card.bounds.left() + px(24.0 * zoom) {
            return None;
        }
        Some((card.id.clone(), fold))
    }
    pub(super) fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.controller.closing() {
            return;
        }
        let zoom = self.controller.snapshot().viewport.zoom;
        match &mut self.canvas.drag {
            Some(Drag::Pan(_, last)) => {
                let delta = Point::new(
                    f32::from(event.position.x - last.x),
                    f32::from(event.position.y - last.y),
                );
                *last = event.position;
                self.command(Command::Pan(delta), cx);
            }
            Some(Drag::Card(id, start, origin)) => {
                self.canvas.drag_preview = Some((
                    id.clone(),
                    Point::new(
                        origin.x + f32::from(event.position.x - start.x) / zoom,
                        origin.y + f32::from(event.position.y - start.y) / zoom,
                    ),
                ));
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
        if self.canvas.selected.as_ref().is_some_and(|id| {
            !self
                .controller
                .snapshot()
                .cards
                .iter()
                .any(|card| card.id.as_str() == id)
        }) {
            self.canvas.selected = None;
        }
        if matches!(&self.canvas.drag, Some(Drag::Card(id, ..)) if !self.controller.snapshot().cards.iter().any(|card| card.id.as_str() == id))
        {
            self.canvas.drag = None;
            self.canvas.drag_preview = None;
        }
    }
    pub(super) fn finish_drag(&mut self, cx: &mut Context<Self>) {
        if matches!(self.canvas.drag.take(), Some(Drag::Card(..)))
            && let Some((id, position)) = self.canvas.drag_preview.take()
        {
            self.command(Command::MoveCard { id, position }, cx);
        }
        self.layout_activity(cx);
        cx.notify();
    }
    pub(super) fn release(&mut self, button: MouseButton, cx: &mut Context<Self>) {
        let matches = match &self.canvas.drag {
            Some(Drag::Pan(start, _)) => *start == button,
            Some(Drag::Card(..)) => button == MouseButton::Left,
            None => false,
        };
        if matches {
            self.finish_drag(cx);
        }
    }
    pub(super) fn cancel_gesture(&mut self, cx: &mut Context<Self>) {
        self.canvas.drag = None;
        self.canvas.drag_preview = None;
        self.layout_activity(cx);
        cx.notify();
    }
    pub(super) fn scroll(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.controller.closing() {
            return;
        }
        self.clear_hover(cx);
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
            self.command(
                Command::Pan(Point::new(
                    if event.modifiers.shift { dy } else { dx },
                    if event.modifiers.shift { 0.0 } else { dy },
                )),
                cx,
            );
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
                self.layout_activity(cx);
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
