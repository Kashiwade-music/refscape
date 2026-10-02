use super::{Command, ExplorerView, LINE, Transition, color, connected_word};
use gpui::{Context, KeyDownEvent, MouseButton, Pixels, div, point, prelude::*, px};
use refscape_model::Position;
use std::time::Duration;

#[derive(Clone, PartialEq)]
pub(super) struct HoverTarget {
    card_id: String,
    position: Position,
    anchor: gpui::Point<Pixels>,
}

impl ExplorerView {
    fn transition_hover_cancel(&mut self, transition: Transition, cx: &mut Context<Self>) {
        self.transition(transition, cx);
    }
    pub(super) fn clear_hover(&mut self, cx: &mut Context<Self>) {
        if self.canvas.context_hover.take().is_some() {
            cx.notify();
        }
        let transition = self.controller.dispatch(Command::CancelHover);
        self.transition_hover_cancel(transition, cx);
        self.hover.task = None;
        self.hover.dismiss_task = None;
        self.hover.pending_target = None;
        if self.hover.target.take().is_some() || self.hover.text.is_some() {
            self.hover.text = None;
            self.hover.scroll.set_offset(point(px(0.0), px(0.0)));
            cx.notify();
        }
    }

    pub(super) fn dismiss_hover(&mut self, cx: &mut Context<Self>) {
        self.hover.pending_target = None;
        if self.hover.text.is_none() {
            self.clear_hover(cx);
        } else if self.hover.dismiss_task.is_none() {
            // Let the pointer cross the space between a word and its popup.
            // Entering either cancels this task, while leaving both closes it.
            self.hover.dismiss_task = Some(cx.spawn(async move |view, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(300))
                    .await;
                let _ = view.update(cx, |view, cx| {
                    let next_target = view.hover.pending_target.take();
                    view.clear_hover(cx);
                    if let Some(target) = next_target {
                        view.request_hover(target, cx);
                    }
                });
            }));
        }
    }

    fn keep_hover(&mut self) {
        self.hover.dismiss_task = None;
        self.hover.pending_target = None;
    }

    fn hover_at(&self, mouse: gpui::Point<Pixels>) -> Option<HoverTarget> {
        let zoom = self.controller.snapshot().viewport.zoom;
        if zoom < 0.65 || !self.canvas.bounds.contains(&mouse) {
            return None;
        }
        let painted = self.canvas.painted.iter().rev().find(|card| {
            card.current(self.controller.snapshot()) && card.bounds.contains(&mouse)
        })?;
        if mouse.y < painted.origin.y {
            return None;
        }
        let row = (f32::from(mouse.y - painted.origin.y) / (LINE * zoom)).floor() as usize;
        if mouse.x < painted.origin.x {
            return None;
        }
        let source_row = painted.row(row)?;
        let line = &source_row.code;
        let source_position = source_row.position?;
        let first_character = source_position.character;
        let position = source_row.source_position(mouse.x - painted.origin.x)?;
        let card = self
            .controller
            .snapshot()
            .cards
            .iter()
            .find(|card| card.id == painted.id)?;
        let (_, word) = connected_word(card, position)?;
        Some(HoverTarget {
            card_id: painted.id.clone(),
            position: Position::new(
                position.line,
                first_character + line.text[..word.start].encode_utf16().count() as u32,
            ),
            anchor: point(
                painted.origin.x + line.x_for_index(word.start),
                painted.origin.y + px((row + 1) as f32 * LINE * zoom),
            ),
        })
    }

    pub(super) fn update_hover(&mut self, mouse: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        if self.requests.busy || self.canvas.drag.is_some() {
            self.clear_hover(cx);
            return;
        }
        if let Some(control) = self.context_control_at(mouse) {
            self.clear_hover(cx);
            self.canvas.context_hover = Some(control);
            cx.notify();
            return;
        }
        if self.canvas.context_hover.take().is_some() {
            cx.notify();
        }
        let target = self.hover_at(mouse);
        if target == self.hover.target {
            self.keep_hover();
            return;
        }
        if self.hover.text.is_some() {
            // Crossing another code word on the way to the panel should not
            // replace the explanation before the pointer can reach it.
            self.dismiss_hover(cx);
            self.hover.pending_target = target;
            return;
        }
        self.clear_hover(cx);
        let Some(target) = target else {
            return;
        };
        self.request_hover(target, cx);
    }

    fn request_hover(&mut self, target: HoverTarget, cx: &mut Context<Self>) {
        self.hover.target = Some(target.clone());
        self.hover.task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            let _ = view.update(cx, |view, cx| {
                if view.hover.target.as_ref() == Some(&target)
                    && !view.controller.busy()
                    && !view.controller.closing()
                {
                    view.command(
                        Command::Hover {
                            card: target.card_id,
                            position: target.position,
                        },
                        cx,
                    );
                }
            });
        }));
    }
    pub(super) fn hover_panel(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let target = self.hover.target.as_ref()?;
        let text = self.hover.text.as_ref()?;
        let palette = &self.controller.snapshot().theme.palette;
        let width = (f32::from(self.canvas.bounds.size.width) - 24.0).clamp(1.0, 460.0);
        let height = (f32::from(self.canvas.bounds.size.height) - 24.0).clamp(1.0, 320.0);
        let left = f32::from(target.anchor.x - self.canvas.bounds.left()).clamp(
            12.0,
            (f32::from(self.canvas.bounds.size.width) - width - 12.0).max(12.0),
        );
        let top = (f32::from(target.anchor.y - self.canvas.bounds.top()) + 6.0).clamp(
            12.0,
            (f32::from(self.canvas.bounds.size.height) - height - 12.0).max(12.0),
        );
        Some(
            div()
                .id("code-hover")
                .debug_selector(|| "code-hover".into())
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(width))
                .max_h(px(height))
                .overflow_y_scroll()
                .track_scroll(&self.hover.scroll)
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(color(&palette.border))
                .bg(color(&palette.surface_alt))
                .text_color(color(&palette.text))
                .text_size(px(12.0))
                .font_family("Consolas")
                .shadow_md()
                .occlude()
                .on_mouse_move(cx.listener(|view, _, _, cx| {
                    view.keep_hover();
                    cx.stop_propagation();
                }))
                .on_hover(cx.listener(|view, hovered, _, cx| {
                    if *hovered {
                        view.keep_hover();
                    } else {
                        view.dismiss_hover(cx);
                    }
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(cx.listener(|view, _, _, cx| {
                    view.keep_hover();
                    cx.stop_propagation();
                }))
                .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        view.clear_hover(cx);
                    }
                }))
                .children(
                    text.lines().map(|line| {
                        div().child(if line.is_empty() { " " } else { line }.to_string())
                    }),
                ),
        )
    }
}
