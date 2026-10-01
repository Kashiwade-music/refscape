use super::*;
use std::time::Duration;

#[derive(Clone, PartialEq)]
pub(super) struct HoverTarget {
    card_id: String,
    position: Position,
    anchor: gpui::Point<Pixels>,
}

impl<L: LanguageService + 'static, R: SessionRepository + 'static> ExplorerView<L, R> {
    pub(super) fn clear_hover(&mut self, cx: &mut Context<Self>) {
        self.hover_task = None;
        self.hover_dismiss_task = None;
        self.hover_pending_target = None;
        if self.hover_target.take().is_some() || self.hover_text.is_some() {
            self.hover_text = None;
            self.hover_scroll.set_offset(point(px(0.0), px(0.0)));
            cx.notify();
        }
    }

    pub(super) fn dismiss_hover(&mut self, cx: &mut Context<Self>) {
        self.hover_pending_target = None;
        if self.hover_text.is_none() {
            self.clear_hover(cx);
        } else if self.hover_dismiss_task.is_none() {
            // Let the pointer cross the space between a word and its popup.
            // Entering either cancels this task, while leaving both closes it.
            self.hover_dismiss_task = Some(cx.spawn(async move |view, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(300))
                    .await;
                let _ = view.update(cx, |view, cx| {
                    let next_target = view.hover_pending_target.take();
                    view.clear_hover(cx);
                    if let Some(target) = next_target {
                        view.request_hover(target, cx);
                    }
                });
            }));
        }
    }

    fn keep_hover(&mut self) {
        self.hover_dismiss_task = None;
        self.hover_pending_target = None;
    }

    fn hover_at(&self, mouse: gpui::Point<Pixels>) -> Option<HoverTarget> {
        let zoom = self.session.viewport.zoom;
        if zoom < 0.65 || !self.bounds.contains(&mouse) {
            return None;
        }
        let painted = self
            .painted
            .iter()
            .rev()
            .find(|card| card.bounds.contains(&mouse))?;
        if mouse.y < painted.origin.y {
            return None;
        }
        let row = (f32::from(mouse.y - painted.origin.y) / (LINE * zoom)).floor() as usize;
        let line = painted.lines.get(row)?;
        let byte = line.index_for_x(mouse.x - painted.origin.x)?;
        let first_character = if row == 0 { painted.first_character } else { 0 };
        let position = Position::new(
            painted.first_line + row as u32,
            first_character + line.text[..byte].encode_utf16().count() as u32,
        );
        let card = self
            .session
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
        if self.busy || self.drag.is_some() {
            self.clear_hover(cx);
            return;
        }
        let target = self.hover_at(mouse);
        if target == self.hover_target {
            self.keep_hover();
            return;
        }
        if self.hover_text.is_some() {
            // Crossing another code word on the way to the panel should not
            // replace the explanation before the pointer can reach it.
            self.dismiss_hover(cx);
            self.hover_pending_target = target;
            return;
        }
        self.clear_hover(cx);
        let Some(target) = target else {
            return;
        };
        self.request_hover(target, cx);
    }

    fn request_hover(&mut self, target: HoverTarget, cx: &mut Context<Self>) {
        self.hover_target = Some(target.clone());
        let explorer = self.explorer.clone();
        self.hover_task = Some(cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            let request = target.clone();
            // Hover is read-only and never queues behind project loading or navigation.
            let result = cx
                .background_executor()
                .spawn(async move {
                    match explorer.try_lock() {
                        Ok(mut explorer) => explorer.hover(&request.card_id, request.position),
                        Err(_) => Ok(None),
                    }
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                if view.hover_target.as_ref() != Some(&target) || view.busy || view.closing {
                    return;
                }
                view.hover_text = match result {
                    Ok(text) => text.filter(|text| !text.trim().is_empty()),
                    Err(error) => Some(format!("Hover information unavailable: {error}")),
                };
                cx.notify();
            });
        }));
    }

    pub(super) fn hover_panel(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let target = self.hover_target.as_ref()?;
        let text = self.hover_text.as_ref()?;
        let palette = &self.session.theme.palette;
        let width = (f32::from(self.bounds.size.width) - 24.0).clamp(1.0, 460.0);
        let height = (f32::from(self.bounds.size.height) - 24.0).clamp(1.0, 320.0);
        let left = f32::from(target.anchor.x - self.bounds.left()).clamp(
            12.0,
            (f32::from(self.bounds.size.width) - width - 12.0).max(12.0),
        );
        let top = (f32::from(target.anchor.y - self.bounds.top()) + 6.0).clamp(
            12.0,
            (f32::from(self.bounds.size.height) - height - 12.0).max(12.0),
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
                .track_scroll(&self.hover_scroll)
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
