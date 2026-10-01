//! Native controls and view composition.
use super::*;

impl<L: LanguageService + 'static, R: SessionRepository + 'static> ExplorerView<L, R> {
    pub(super) fn button(
        &self,
        label: &str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        div()
            .id(label.to_string())
            .px_3()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .bg(color(&self.session.theme.palette.surface_alt))
            .text_size(px(12.0))
            .hover(|style| style.opacity(0.75))
            .child(label.to_string())
            .on_click(cx.listener(move |view, _, _, cx| {
                if !view.requests.closing {
                    action(view, cx);
                }
            }))
    }
}

impl<L: LanguageService + 'static, R: SessionRepository + 'static> Render for ExplorerView<L, R> {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.session.theme.palette.clone();
        let entity = cx.entity();
        let query_entity = entity.clone();
        let query = self.search.query.clone();
        let selection = self.search.selection.clone();
        let focused = self.search.focused;
        let query_palette = palette.clone();
        let session = self.session.clone();
        let selected = self.canvas.selected.clone();
        let preview = self.canvas.drag_preview.clone();
        let root = session.project_root.clone();
        let lower = query.to_lowercase();
        let files: Vec<_> = self
            .project
            .files
            .iter()
            .filter(|p| lower.is_empty() || p.to_string_lossy().to_lowercase().contains(&lower))
            .cloned()
            .collect();
        let symbols = self.project.symbols.clone();
        let toolbar = div()
            .min_h(px(60.0))
            .flex_shrink_0()
            .px_4()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(color(&palette.border))
            .child(
                div()
                    .flex_shrink_0()
                    .max_w(px(220.0))
                    .overflow_hidden()
                    .child(div().text_size(px(20.0)).child("Refscape"))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(color(&palette.muted))
                            .child(display_path(&root)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .justify_end()
                    .flex_wrap()
                    .gap_2()
                    .child(self.button("Open project", cx, |v, cx| v.pick_project(cx)))
                    .child(self.button("Build settings", cx, |v, cx| {
                        v.pick_compilation_database(cx)
                    }))
                    .child(self.button("Save", cx, |v, cx| v.save(cx)))
                    .child(self.button("Save as", cx, |v, cx| v.pick_session(true, cx)))
                    .child(self.button("Open session", cx, |v, cx| v.pick_session(false, cx)))
                    .child(self.button("Fit · 0", cx, |v, cx| v.fit(cx)))
                    .child(self.button("Arrange", cx, |v, cx| v.arrange_layout(cx)))
                    .children(
                        self.layout
                            .can_undo
                            .then(|| self.button("Undo layout", cx, |v, cx| v.undo_layout(cx))),
                    )
                    .child(self.button(
                        &format!("Theme: {}", self.session.theme.name),
                        cx,
                        |v, cx| v.cycle_theme(cx),
                    )),
            );
        let sidebar = div().w(px(280.0)).h_full().flex_shrink_0().flex().flex_col().p_3().gap_3().bg(color(&palette.surface)).border_r_1().border_color(color(&palette.border))
            .children((!root.as_os_str().is_empty()).then(||
                div().text_size(px(10.0)).text_color(color(&palette.muted))
                    .child(project_settings_label(&session.project_options))))
            .child(div().text_size(px(11.0)).text_color(color(&palette.muted)).child("FIND A STARTING POINT"))
            .child(div().id("search").h(px(36.0)).w_full().border_1().rounded_md().overflow_hidden().border_color(color(if focused { &palette.accent } else { &palette.border }))
                .on_mouse_down(MouseButton::Left, cx.listener(|v, event: &MouseDownEvent, window, cx| { v.search.focused = true; window.focus(&v.focus, cx); if let (Some(bounds), Some(line)) = (v.search.bounds, &v.search.line) { let index = line.closest_index_for_x(event.position.x - bounds.left() - px(8.0)).min(v.search.query.len()); v.search.selection = index..index; } cx.notify(); }))
                .child(canvas(move |_, _, _| {}, move |bounds, _, window, cx| {
                    let text = if query.is_empty() { "File filter / symbol search".to_string() } else { query.clone() };
                    let origin = point(bounds.left() + px(8.0), bounds.top() + px(8.0));
                    let run = TextRun { len: text.len(), font: gpui::font("Segoe UI"), color: color(if query.is_empty() { &query_palette.muted } else { &query_palette.text }), background_color: None, underline: None, strikethrough: None };
                    let line = window.text_system().shape_line(text.into(), px(12.0), &[run], None);
                    if focused {
                        window.handle_input(&query_entity.read(cx).focus, ElementInputHandler::new(bounds, query_entity.clone()), cx);
                        if !query.is_empty() && !selection.is_empty() { window.paint_quad(fill(Bounds::new(point(origin.x + line.x_for_index(selection.start), origin.y), size(line.x_for_index(selection.end) - line.x_for_index(selection.start), px(20.0))), color(&query_palette.accent).opacity(0.3))); }
                        let x = if query.is_empty() { px(0.0) } else { line.x_for_index(selection.end) };
                        window.paint_quad(fill(Bounds::new(point(origin.x + x, origin.y), size(px(1.0), px(18.0))), color(&query_palette.accent)));
                    }
                    let _ = line.paint(origin, px(20.0), TextAlign::Left, None, window, cx);
                    query_entity.update(cx, |v, _| { v.search.bounds = Some(bounds); v.search.line = Some(line); });
                }).size_full()))
            .child(self.button("Search symbols · Enter", cx, |v, cx| v.search(cx)))
            .child(div().id("files").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().gap_1()
                .child(div().py_2().text_size(px(11.0)).text_color(color(&palette.muted)).child(format!("SYMBOLS · {}", symbols.len())))
                .children(symbols.into_iter().take(250).enumerate().map(|(index, symbol)| {
                    let source = symbol.clone();
                    div().id(("symbol", index)).p_2().rounded_md().cursor_pointer().hover(|style| style.bg(color(&palette.surface_alt)))
                        .child(div().text_size(px(12.0)).child(symbol.name)).child(div().text_size(px(10.0)).text_color(color(&palette.muted)).child(format!("{} · {}:{}", symbol.kind, symbol.path.strip_prefix(&root).unwrap_or(&symbol.path).display(), symbol.selection_range.start.line + 1)))
                        .on_click(cx.listener(move |v, _, _, cx| v.toggle_symbol(source.clone(), cx)))
                }))
                .child(div().py_2().text_size(px(11.0)).text_color(color(&palette.muted)).child(format!("FILES · {}", files.len())))
                .children(files.into_iter().take(1000).enumerate().map(|(index, path)| {
                    let title = path.strip_prefix(&root).unwrap_or(&path).display().to_string();
                    div().id(("file", index)).p_2().text_size(px(11.0)).rounded_md().cursor_pointer().hover(|style| style.bg(color(&palette.surface_alt))).child(title).on_click(cx.listener(move |v, _, _, cx| v.add_file(path.clone(), cx)))
                })))
            .child(div().text_size(px(10.0)).text_color(color(&palette.muted)).child("Click variable: highlight + type\nClick function / type: definition\nAlt + click: definition\nRight-click: references\nEsc: clear highlight\nDrag header: move card\nDrag canvas / wheel: pan\nCtrl + wheel: zoom\nCtrl + S: save · Delete: remove"))
            .children(self.canvas.inspection.as_ref().and_then(|inspection| inspection.description.as_ref()).map(|description| {
                div().mt_3().p_2().rounded_md().bg(color(&palette.surface_alt))
                    .text_size(px(11.0)).child(description.lines().take(6).collect::<Vec<_>>().join("\n"))
            }));
        let inspection = self.canvas.inspection.clone();
        let context_hover = self.canvas.context_hover.clone();
        let workspace = div()
            .id("canvas")
            .relative()
            .flex_1()
            .h_full()
            .overflow_hidden()
            .when(context_hover.is_some(), |canvas| canvas.cursor_pointer())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|v, e, _, cx| v.mouse_down(e, false, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|v, e, _, cx| v.mouse_down(e, true, cx)),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|v, e: &MouseDownEvent, _, cx| {
                    if v.requests.closing {
                        return;
                    }
                    v.clear_hover(cx);
                    v.canvas.drag = Some(Drag::Pan(e.position));
                    v.layout_activity(cx);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|v, e, _, cx| v.mouse_move(e, cx)))
            .on_hover(cx.listener(|v, hovered, _, cx| {
                if !hovered {
                    if v.canvas.context_hover.take().is_some() {
                        cx.notify();
                    }
                    v.dismiss_hover(cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|v, _, _, cx| {
                    v.finish_drag(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|v, _, _, cx| {
                    v.finish_drag(cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|v, _, _, cx| {
                    v.finish_drag(cx);
                }),
            )
            .on_scroll_wheel(cx.listener(|v, e, _, cx| v.scroll(e, cx)))
            .child(
                canvas(
                    move |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let painted = paint_canvas(
                            &session,
                            (selected.as_deref(), context_hover.as_ref()),
                            inspection.as_ref(),
                            bounds,
                            window,
                            cx,
                        );
                        if let Some((id, position)) = &preview
                            && let Some(card) = session.cards.iter().find(|card| &card.id == id)
                        {
                            let mut preview_card = card.clone();
                            preview_card.position = *position;
                            window.paint_quad(quad(
                                card_bounds(&preview_card, &session, bounds),
                                px(7.0 * session.viewport.zoom),
                                color(&session.theme.palette.accent).opacity(0.08),
                                px(2.0),
                                color(&session.theme.palette.accent),
                                Default::default(),
                            ));
                        }
                        entity.update(cx, |v, _| {
                            v.canvas.painted = painted;
                            v.canvas.bounds = bounds;
                        });
                    },
                )
                .size_full(),
            )
            .children(self.hover_panel(cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(color(&palette.background))
            .text_color(color(&palette.text))
            .font_family("Segoe UI")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .child(toolbar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(workspace),
            )
            .child(
                div()
                    .h(px(32.0))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(color(&palette.border))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(if self.requests.error {
                                rgb(0xe76e78).into()
                            } else {
                                color(&palette.muted)
                            })
                            .child(format!(
                                "{}{}",
                                if self.requests.busy { "● " } else { "" },
                                self.requests.status
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(color(&palette.muted))
                            .child(format!(
                                "{}% · {} · {}",
                                (self.session.viewport.zoom * 100.0).round(),
                                if self.session.viewport.zoom < 0.35 {
                                    if self.session.project_options.language == ProjectLanguage::Cpp
                                    {
                                        "PROJECT"
                                    } else {
                                        "CRATES"
                                    }
                                } else if self.session.viewport.zoom < 0.65 {
                                    "MODULES"
                                } else {
                                    "CODE"
                                },
                                display_path(&self.project.session_path)
                            )),
                    ),
            )
    }
}

pub(super) fn color(value: &str) -> gpui::Hsla {
    rgb(u32::from_str_radix(value.trim_start_matches('#'), 16).unwrap_or(0x808080)).into()
}

pub(super) fn project_settings_label(options: &ProjectOptions) -> String {
    match options.language {
        ProjectLanguage::Auto => "Language: automatic".into(),
        ProjectLanguage::Rust => "Rust · rust-analyzer".into(),
        ProjectLanguage::TypeScript => "TypeScript / JavaScript / React · tsserver".into(),
        ProjectLanguage::Cpp => match &options.compilation_database {
            Some(path) => format!("C/C++ · clangd\nBuild settings: {}", display_path(path)),
            None => "C/C++ · clangd · project/default flags\nReferences and symbol search may be incomplete. Select Build settings to load compile_commands.json.".into(),
        },
    }
}

pub(super) fn display_path(path: &std::path::Path) -> String {
    let value = path.display().to_string();
    if let Some(unc) = value.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        value.strip_prefix("\\\\?\\").unwrap_or(&value).to_string()
    }
}
