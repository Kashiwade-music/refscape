//! Native spatial explorer; language and persistence operations run off the UI thread.
mod input;
#[cfg(test)]
mod tests;

use gpui::{
    App, Bounds, Context, ElementInputHandler, FocusHandle, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, PathBuilder, PathPromptOptions, Pixels, Render, ScrollDelta,
    ScrollWheelEvent, ShapedLine, TextAlign, TextRun, Window, canvas, div, fill, point, prelude::*,
    px, quad, rgb, size,
};
use refscape_application::{Explorer, LanguageService, SessionRepository};
use refscape_model::{CodeCard, Palette, Point, Position, Session, Symbol, Theme};
use std::{
    ops::Range,
    path::PathBuf,
    sync::{Arc, Mutex},
};

const HEADER: f32 = 52.0;
const LINE: f32 = 20.0;
type Job<L, R> = Box<dyn FnOnce(&mut Explorer<L, R>) -> Result<Output, String> + Send>;
#[derive(Default)]
struct Output {
    files: Option<Vec<PathBuf>>,
    symbols: Option<Vec<Symbol>>,
    reset: bool,
    message: Option<String>,
    session_path: Option<PathBuf>,
    protect_session: bool,
}
struct PaintedCard {
    id: String,
    bounds: Bounds<Pixels>,
    lines: Vec<ShapedLine>,
    first_line: u32,
    first_character: u32,
    origin: gpui::Point<Pixels>,
}
struct CodeConnection {
    source_card: String,
    underline: Bounds<Pixels>,
    start: gpui::Point<Pixels>,
    exit: gpui::Point<Pixels>,
    end: gpui::Point<Pixels>,
}
enum Drag {
    Pan(gpui::Point<Pixels>),
    Card(String, gpui::Point<Pixels>, Point),
}

/// GPUI view constructed by the composition root with concrete adapters.
pub struct ExplorerView<L: LanguageService + 'static, R: SessionRepository + 'static> {
    explorer: Arc<Mutex<Explorer<L, R>>>,
    session: Session,
    session_path: PathBuf,
    themes: Vec<Theme>,
    files: Vec<PathBuf>,
    symbols: Vec<Symbol>,
    query: String,
    query_selection: Range<usize>,
    query_marked: Option<Range<usize>>,
    query_bounds: Option<Bounds<Pixels>>,
    query_line: Option<ShapedLine>,
    search_focus: bool,
    focus: FocusHandle,
    busy: bool,
    status: String,
    error: bool,
    selected: Option<String>,
    drag: Option<Drag>,
    painted: Vec<PaintedCard>,
    bounds: Bounds<Pixels>,
    closing: bool,
    autosave: bool,
}

impl<L: LanguageService + 'static, R: SessionRepository + 'static> ExplorerView<L, R> {
    /// Indexing and initial session restoration begin after the window is created.
    pub fn new(
        explorer: Explorer<L, R>,
        session_path: PathBuf,
        custom_themes: Vec<Theme>,
        initial_project: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let session = explorer.session().clone();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut themes = vec![Theme::dark(), Theme::light()];
        for theme in custom_themes {
            if !themes.contains(&theme) {
                themes.push(theme);
            }
        }
        let mut view = Self {
            explorer: Arc::new(Mutex::new(explorer)),
            session,
            session_path,
            themes,
            files: vec![],
            symbols: vec![],
            query: String::new(),
            query_selection: 0..0,
            query_marked: None,
            query_bounds: None,
            query_line: None,
            search_focus: false,
            focus,
            busy: false,
            status: "Open a Rust project to start exploring.".into(),
            error: false,
            selected: None,
            drag: None,
            painted: vec![],
            bounds: Bounds::default(),
            closing: false,
            autosave: true,
        };
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |view, cx| view.close(window, cx))
                .unwrap_or(true)
        });
        if let Some(path) = initial_project {
            view.open_project(path, cx);
        } else if !view.session.project_root.as_os_str().is_empty() {
            view.run_job(
                "Loading project files…",
                Box::new(|explorer| {
                    Ok(Output {
                        files: Some(explorer.files()?),
                        ..Default::default()
                    })
                }),
                cx,
            );
        }
        view
    }

    fn run_job(&mut self, label: &str, job: Job<L, R>, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        if self.busy {
            self.status = "A request is running. Try again when it finishes.".into();
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = false;
        self.status = label.into();
        let explorer = self.explorer.clone();
        let viewport = self.session.viewport;
        let positions = self
            .session
            .cards
            .iter()
            .map(|c| (c.id.clone(), c.position))
            .collect();
        let task = cx.background_executor().spawn(async move {
            let mut explorer = explorer
                .lock()
                .map_err(|_| "Explorer lock poisoned".to_string())?;
            explorer.sync_canvas(viewport, positions)?;
            let result = job(&mut explorer);
            Ok::<_, String>((result, explorer.session().clone()))
        });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                view.busy = false;
                match result {
                    Ok((Ok(output), mut session)) => {
                        if !output.reset {
                            session.viewport = view.session.viewport;
                            for card in &mut session.cards {
                                if let Some(old) =
                                    view.session.cards.iter().find(|old| old.id == card.id)
                                {
                                    card.position = old.position;
                                }
                            }
                        }
                        view.session = session;
                        if !view.themes.contains(&view.session.theme) {
                            view.themes.push(view.session.theme.clone());
                        }
                        if let Some(files) = output.files {
                            view.files = files;
                        }
                        if let Some(symbols) = output.symbols {
                            view.symbols = symbols;
                        }
                        if let Some(path) = output.session_path {
                            view.session_path = path;
                            view.autosave = !output.protect_session;
                        }
                        if output.reset {
                            view.symbols.clear();
                            view.query.clear();
                            view.query_selection = 0..0;
                            view.query_marked = None;
                            view.selected = None;
                            view.drag = None;
                        }
                        view.status = output.message.unwrap_or_else(|| {
                            format!(
                                "{} cards · {} connections · Ready",
                                view.session.cards.len(),
                                view.session.connections.len()
                            )
                        });
                        view.error = false;
                    }
                    Ok((Err(error), mut session)) => {
                        if session.project_root == view.session.project_root {
                            session.viewport = view.session.viewport;
                            for card in &mut session.cards {
                                if let Some(old) =
                                    view.session.cards.iter().find(|old| old.id == card.id)
                                {
                                    card.position = old.position;
                                }
                            }
                        }
                        view.session = session;
                        view.status = error;
                        view.error = true;
                    }
                    Err(error) => {
                        view.status = error;
                        view.error = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn open_project(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let session_path = if self.session_path.as_os_str().is_empty() {
            path.join(".refscape/session.json")
        } else {
            self.session_path.clone()
        };
        self.open_project_with_session(path, session_path, cx);
    }

    fn open_project_with_session(
        &mut self,
        path: PathBuf,
        session_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let previous = if self.autosave && !self.session.project_root.as_os_str().is_empty() {
            Some(self.session_path.clone())
        } else {
            None
        };
        self.symbols.clear();
        self.query.clear();
        self.query_selection = 0..0;
        self.query_marked = None;
        self.run_job(
            "Starting rust-analyzer and indexing project…",
            Box::new(move |explorer| {
                if let Some(previous) = previous { explorer.save_session(&previous)?; }
                explorer.open_project(&path)?;
                let project_root = explorer.session().project_root.clone();
                let mut message = None;
                if session_path.is_file() {
                    let active_theme = explorer.session().theme.clone();
                    if let Err(error) = explorer.load_session(&session_path) {
                        message = Some(format!("Project opened; session restore failed: {error}"));
                    }
                    if explorer.session().project_root != project_root {
                        explorer.open_project(&path)?;
                        explorer.set_theme(active_theme.clone())?;
                        message = Some("Project opened; saved session belongs to another project and was not restored.".into());
                    }
                    if active_theme != Theme::dark() && active_theme != Theme::light() {
                        explorer.set_theme(active_theme)?;
                    }
                }
                Ok(Output {
                    files: Some(explorer.files()?),
                    reset: true,
                    protect_session: message.is_some(),
                    message,
                    session_path: Some(session_path),
                    ..Default::default()
                })
            }),
            cx,
        );
    }

    fn pick_project(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.closing {
            return;
        }
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Rust project".into()),
        });
        cx.spawn(async move |view, cx| match picker.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = view.update(cx, |view, cx| {
                        let session_path = if view.session.project_root.as_os_str().is_empty()
                            && !view.session_path.as_os_str().is_empty()
                        {
                            view.session_path.clone()
                        } else {
                            path.join(".refscape/session.json")
                        };
                        view.open_project_with_session(path, session_path, cx);
                    });
                }
            }
            Ok(Ok(None)) => {}
            result => {
                let _ = view.update(cx, |view, cx| {
                    view.status = format!("Project picker failed: {result:?}");
                    view.error = true;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.query.clone();
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

    fn save(&mut self, cx: &mut Context<Self>) {
        let path = self.session_path.clone();
        self.run_job(
            "Saving session…",
            Box::new(move |explorer| {
                explorer.save_session(&path)?;
                Ok(Output {
                    message: Some(format!("Session saved: {}", display_path(&path))),
                    session_path: Some(path),
                    ..Default::default()
                })
            }),
            cx,
        );
    }

    fn pick_session(&mut self, save: bool, cx: &mut Context<Self>) {
        if self.busy || self.closing {
            return;
        }
        if save {
            let picker = cx.prompt_for_new_path(
                self.session_path
                    .parent()
                    .unwrap_or(std::path::Path::new(".")),
                Some("refscape-session.json"),
            );
            cx.spawn(async move |view, cx| match picker.await {
                Ok(Ok(Some(path))) => {
                    let _ = view.update(cx, |view, cx| {
                        view.run_job(
                            "Saving session…",
                            Box::new(move |explorer| {
                                explorer.save_session(&path)?;
                                Ok(Output {
                                    session_path: Some(path),
                                    message: Some("Session saved.".into()),
                                    ..Default::default()
                                })
                            }),
                            cx,
                        );
                    });
                }
                Ok(Ok(None)) => {}
                result => {
                    let _ = view.update(cx, |view, cx| {
                        view.status = format!("Session picker failed: {result:?}");
                        view.error = true;
                        cx.notify();
                    });
                }
            })
            .detach();
        } else {
            let picker = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Open Refscape session".into()),
            });
            cx.spawn(async move |view, cx| match picker.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = view.update(cx, |view, cx| {
                            let previous = if view.autosave
                                && !view.session.project_root.as_os_str().is_empty()
                                && view.session_path != path
                            {
                                Some(view.session_path.clone())
                            } else {
                                None
                            };
                            view.run_job(
                                "Restoring session…",
                                Box::new(move |explorer| {
                                    if let Some(previous) = previous {
                                        explorer.save_session(&previous)?;
                                    }
                                    explorer.load_session(&path)?;
                                    Ok(Output {
                                        files: Some(explorer.files()?),
                                        reset: true,
                                        session_path: Some(path),
                                        ..Default::default()
                                    })
                                }),
                                cx,
                            );
                        });
                    }
                }
                Ok(Ok(None)) => {}
                result => {
                    let _ = view.update(cx, |view, cx| {
                        view.status = format!("Session picker failed: {result:?}");
                        view.error = true;
                        cx.notify();
                    });
                }
            })
            .detach();
        }
    }

    fn insertion_point(&self) -> Point {
        self.session
            .viewport
            .screen_to_world(Point::new(100.0, 80.0))
    }
    fn add_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let position = self.insertion_point();
        self.run_job(
            "Opening source file…",
            Box::new(move |explorer| {
                explorer.add_file(&path, position)?;
                Ok(Output {
                    symbols: Some(explorer.symbols(&path)?),
                    ..Default::default()
                })
            }),
            cx,
        );
    }
    fn add_symbol(&mut self, symbol: Symbol, cx: &mut Context<Self>) {
        let position = self.insertion_point();
        self.run_job(
            "Opening symbol…",
            Box::new(move |explorer| {
                explorer.add_symbol(symbol, position)?;
                Ok(Output::default())
            }),
            cx,
        );
    }
    fn remove_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected.take() {
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
    fn cycle_theme(&mut self, cx: &mut Context<Self>) {
        let index = self
            .themes
            .iter()
            .position(|t| *t == self.session.theme)
            .unwrap_or(0);
        let theme = self.themes[(index + 1) % self.themes.len()].clone();
        self.run_job(
            "Applying theme…",
            Box::new(move |explorer| {
                explorer.set_theme(theme)?;
                Ok(Output::default())
            }),
            cx,
        );
    }
    fn zoom(&mut self, factor: f32, anchor: Point, cx: &mut Context<Self>) {
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
    fn fit(&mut self, cx: &mut Context<Self>) {
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
        let zoom = ((f32::from(self.bounds.size.width).max(600.0) - 80.0)
            / (right - left).max(1.0))
        .min((f32::from(self.bounds.size.height).max(400.0) - 80.0) / (bottom - top).max(1.0))
        .clamp(0.15, 2.0);
        self.session.viewport.zoom = zoom;
        self.session.viewport.offset = Point::new(40.0 - left * zoom, 40.0 - top * zoom);
        cx.notify();
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, references: bool, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.search_focus = false;
        for card in self.painted.iter().rev() {
            if !card.bounds.contains(&event.position) {
                continue;
            }
            let id = card.id.clone();
            self.selected = Some(id.clone());
            let zoom = self.session.viewport.zoom;
            if f32::from(event.position.y - card.bounds.top()) < HEADER * zoom && !references {
                if event.position.x > card.bounds.right() - px(28.0 * zoom) {
                    self.remove_selected(cx);
                } else if let Some(source) = self.session.cards.iter().find(|c| c.id == id) {
                    self.drag = Some(Drag::Card(id, event.position, source.position));
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
            if let Some(line) = card.lines.get(line_index)
                && let Some(byte) = line.index_for_x(event.position.x - card.origin.x)
            {
                let position = Position::new(
                    card.first_line + line_index as u32,
                    line.text[..byte].encode_utf16().count() as u32
                        + if line_index == 0 {
                            card.first_character
                        } else {
                            0
                        },
                );
                self.run_job(
                    if references {
                        "Finding references…"
                    } else {
                        "Finding definitions…"
                    },
                    Box::new(move |explorer| {
                        let found = if references {
                            explorer.expand_references(&id, position)?
                        } else {
                            explorer.expand_definition(&id, position)?
                        };
                        Ok(Output {
                            message: if found.is_empty() {
                                Some(
                                    "rust-analyzer returned no locations for this position.".into(),
                                )
                            } else {
                                None
                            },
                            ..Default::default()
                        })
                    }),
                    cx,
                );
            }
            cx.notify();
            return;
        }
        self.selected = None;
        if !references {
            self.drag = Some(Drag::Pan(event.position));
        }
        cx.notify();
    }
    fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        match &mut self.drag {
            Some(Drag::Pan(last)) => {
                self.session.viewport.offset.x += f32::from(event.position.x - last.x);
                self.session.viewport.offset.y += f32::from(event.position.y - last.y);
                *last = event.position;
            }
            Some(Drag::Card(id, start, origin)) => {
                if let Some(card) = self.session.cards.iter_mut().find(|c| c.id == *id) {
                    card.position = Point::new(
                        origin.x
                            + f32::from(event.position.x - start.x) / self.session.viewport.zoom,
                        origin.y
                            + f32::from(event.position.y - start.y) / self.session.viewport.zoom,
                    );
                }
            }
            None => return,
        }
        cx.notify();
    }
    fn scroll(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(p) => (p.x * 24.0, p.y * 24.0),
        };
        if event.modifiers.control || event.modifiers.platform {
            self.zoom(
                (dy * 0.004).exp(),
                Point::new(
                    f32::from(event.position.x - self.bounds.left()),
                    f32::from(event.position.y - self.bounds.top()),
                ),
                cx,
            );
        } else {
            self.session.viewport.offset.x += if event.modifiers.shift { dy } else { dx };
            self.session.viewport.offset.y += if event.modifiers.shift { 0.0 } else { dy };
            cx.notify();
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform {
            match key {
                "s" if modifiers.shift => self.pick_session(true, cx),
                "s" => self.save(cx),
                "o" if modifiers.shift => self.pick_project(cx),
                "o" => self.pick_session(false, cx),
                "f" | "p" => {
                    self.search_focus = true;
                    self.query_selection = 0..self.query.len();
                    cx.notify();
                }
                "a" if self.search_focus => {
                    self.query_selection = 0..self.query.len();
                    cx.notify();
                }
                "v" if self.search_focus => {
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
                self.search_focus = false;
                self.drag = None;
                cx.notify();
            }
            "enter" if self.search_focus => self.search(cx),
            "backspace" if self.search_focus => {
                let range = if self.query_selection.is_empty() {
                    self.query[..self.query_selection.end]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0)..self.query_selection.end
                } else {
                    self.query_selection.clone()
                };
                self.replace_query(Some(range), "", cx);
            }
            "delete" if self.search_focus => {
                let start = self.query_selection.start;
                let end = if self.query_selection.is_empty() {
                    self.query[start..]
                        .chars()
                        .next()
                        .map(|ch| start + ch.len_utf8())
                        .unwrap_or(start)
                } else {
                    self.query_selection.end
                };
                self.replace_query(Some(start..end), "", cx);
            }
            "left" | "right" | "home" | "end" if self.search_focus => {
                let index = match key {
                    "home" => 0,
                    "end" => self.query.len(),
                    "left" => self.query[..self.query_selection.start]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0),
                    _ => self.query[self.query_selection.end..]
                        .chars()
                        .next()
                        .map(|ch| self.query_selection.end + ch.len_utf8())
                        .unwrap_or(self.query.len()),
                };
                self.query_selection = index..index;
                cx.notify();
            }
            "delete" | "backspace" => self.remove_selected(cx),
            "0" if !self.search_focus => self.fit(cx),
            "+" | "=" if !self.search_focus => self.zoom(1.15, Point::new(300.0, 200.0), cx),
            "-" if !self.search_focus => self.zoom(1.0 / 1.15, Point::new(300.0, 200.0), cx),
            _ => {}
        }
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.busy {
            self.status = "A request is running. Close again after it finishes so the complete session can be saved.".into();
            cx.notify();
            return false;
        }
        if self.session.project_root.as_os_str().is_empty() || !self.autosave {
            return true;
        }
        if self.closing {
            return false;
        }
        self.closing = true;
        self.status = "Saving session before closing…".into();
        self.error = false;
        let explorer = self.explorer.clone();
        let path = self.session_path.clone();
        let viewport = self.session.viewport;
        let positions = self
            .session
            .cards
            .iter()
            .map(|card| (card.id.clone(), card.position))
            .collect();
        let task = cx.background_executor().spawn(async move {
            let mut explorer = explorer
                .lock()
                .map_err(|_| "Explorer lock poisoned".to_string())?;
            explorer.sync_canvas(viewport, positions)?;
            explorer.save_session(&path)
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = cx.update(|window, cx| match result {
                Ok(()) => {
                    window.remove_window();
                }
                Err(error) => {
                    let _ = view.update(cx, |view, cx| {
                        view.closing = false;
                        view.status = format!(
                            "Session save failed: {error}. Use Save as to choose another location."
                        );
                        view.error = true;
                        cx.notify();
                    });
                }
            });
        })
        .detach();
        cx.notify();
        false
    }

    fn button(
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
                if !view.closing {
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
        let query = self.query.clone();
        let selection = self.query_selection.clone();
        let focused = self.search_focus;
        let query_palette = palette.clone();
        let session = self.session.clone();
        let selected = self.selected.clone();
        let root = session.project_root.clone();
        let lower = query.to_lowercase();
        let files: Vec<_> = self
            .files
            .iter()
            .filter(|p| lower.is_empty() || p.to_string_lossy().to_lowercase().contains(&lower))
            .cloned()
            .collect();
        let symbols = self.symbols.clone();
        let toolbar = div()
            .h(px(60.0))
            .flex_shrink_0()
            .px_4()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(color(&palette.border))
            .child(
                div()
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
                    .gap_2()
                    .child(self.button("Open project", cx, |v, cx| v.pick_project(cx)))
                    .child(self.button("Save", cx, |v, cx| v.save(cx)))
                    .child(self.button("Save as", cx, |v, cx| v.pick_session(true, cx)))
                    .child(self.button("Open session", cx, |v, cx| v.pick_session(false, cx)))
                    .child(self.button("Fit · 0", cx, |v, cx| v.fit(cx)))
                    .child(self.button(
                        &format!("Theme: {}", self.session.theme.name),
                        cx,
                        |v, cx| v.cycle_theme(cx),
                    )),
            );
        let sidebar = div().w(px(280.0)).h_full().flex_shrink_0().flex().flex_col().p_3().gap_3().bg(color(&palette.surface)).border_r_1().border_color(color(&palette.border))
            .child(div().text_size(px(11.0)).text_color(color(&palette.muted)).child("FIND A STARTING POINT"))
            .child(div().id("search").h(px(36.0)).w_full().border_1().rounded_md().overflow_hidden().border_color(color(if focused { &palette.accent } else { &palette.border }))
                .on_mouse_down(MouseButton::Left, cx.listener(|v, event: &MouseDownEvent, window, cx| { v.search_focus = true; window.focus(&v.focus, cx); if let (Some(bounds), Some(line)) = (v.query_bounds, &v.query_line) { let index = line.closest_index_for_x(event.position.x - bounds.left() - px(8.0)).min(v.query.len()); v.query_selection = index..index; } cx.notify(); }))
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
                    query_entity.update(cx, |v, _| { v.query_bounds = Some(bounds); v.query_line = Some(line); });
                }).size_full()))
            .child(self.button("Search symbols · Enter", cx, |v, cx| v.search(cx)))
            .child(div().id("files").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().gap_1()
                .child(div().py_2().text_size(px(11.0)).text_color(color(&palette.muted)).child(format!("SYMBOLS · {}", symbols.len())))
                .children(symbols.into_iter().take(250).enumerate().map(|(index, symbol)| {
                    let source = symbol.clone();
                    div().id(("symbol", index)).p_2().rounded_md().cursor_pointer().hover(|style| style.bg(color(&palette.surface_alt)))
                        .child(div().text_size(px(12.0)).child(symbol.name)).child(div().text_size(px(10.0)).text_color(color(&palette.muted)).child(format!("{} · {}:{}", symbol.kind, symbol.path.strip_prefix(&root).unwrap_or(&symbol.path).display(), symbol.selection_range.start.line + 1)))
                        .on_click(cx.listener(move |v, _, _, cx| v.add_symbol(source.clone(), cx)))
                }))
                .child(div().py_2().text_size(px(11.0)).text_color(color(&palette.muted)).child(format!("FILES · {}", files.len())))
                .children(files.into_iter().take(1000).enumerate().map(|(index, path)| {
                    let title = path.strip_prefix(&root).unwrap_or(&path).display().to_string();
                    div().id(("file", index)).p_2().text_size(px(11.0)).rounded_md().cursor_pointer().hover(|style| style.bg(color(&palette.surface_alt))).child(title).on_click(cx.listener(move |v, _, _, cx| v.add_file(path.clone(), cx)))
                })))
            .child(div().text_size(px(10.0)).text_color(color(&palette.muted)).child("Click source: definition\nRight-click: references\nDrag header: move card\nDrag canvas / wheel: pan\nCtrl + wheel: zoom\nCtrl + S: save · Delete: remove"));
        let workspace = div()
            .id("canvas")
            .relative()
            .flex_1()
            .h_full()
            .overflow_hidden()
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
                    v.drag = Some(Drag::Pan(e.position));
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|v, e, _, cx| v.mouse_move(e, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|v, _, _, cx| {
                    v.drag = None;
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|v, _, _, cx| {
                    v.drag = None;
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|v, _, _, cx| {
                    v.drag = None;
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|v, e, _, cx| v.scroll(e, cx)))
            .child(
                canvas(
                    move |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let painted =
                            paint_canvas(&session, selected.as_deref(), bounds, window, cx);
                        entity.update(cx, |v, _| {
                            v.painted = painted;
                            v.bounds = bounds;
                        });
                    },
                )
                .size_full(),
            );
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
                            .text_color(if self.error {
                                rgb(0xe76e78).into()
                            } else {
                                color(&palette.muted)
                            })
                            .child(format!(
                                "{}{}",
                                if self.busy { "● " } else { "" },
                                self.status
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
                                    "CRATES"
                                } else if self.session.viewport.zoom < 0.65 {
                                    "MODULES"
                                } else {
                                    "CODE"
                                },
                                display_path(&self.session_path)
                            )),
                    ),
            )
    }
}

fn color(value: &str) -> gpui::Hsla {
    rgb(u32::from_str_radix(value.trim_start_matches('#'), 16).unwrap_or(0x808080)).into()
}

fn display_path(path: &std::path::Path) -> String {
    let value = path.display().to_string();
    if let Some(unc) = value.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        value.strip_prefix("\\\\?\\").unwrap_or(&value).to_string()
    }
}
fn text(
    text: String,
    origin: gpui::Point<Pixels>,
    font_size: f32,
    color: gpui::Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let run = TextRun {
        len: text.len(),
        font: gpui::font("Segoe UI"),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text.into(), px(font_size), &[run], None);
    let _ = line.paint(
        origin,
        px(font_size * 1.45),
        TextAlign::Left,
        None,
        window,
        cx,
    );
}
fn card_height(card: &CodeCard) -> f32 {
    card.height
        .max(HEADER + card.source.code.lines().count() as f32 * LINE + 24.0)
}
fn card_bounds(card: &CodeCard, session: &Session, canvas: Bounds<Pixels>) -> Bounds<Pixels> {
    let position = session.viewport.world_to_screen(card.position);
    Bounds::new(
        point(
            canvas.left() + px(position.x),
            canvas.top() + px(position.y),
        ),
        size(
            px(card.width * session.viewport.zoom),
            px(card_height(card) * session.viewport.zoom),
        ),
    )
}

/// Locate the displayed word using the server's absolute UTF-16 token range.
/// Without semantic tokens, use ordinary text word selection, never code analysis.
fn connected_word(card: &CodeCard, position: Position) -> Option<(usize, Range<usize>)> {
    if !card.source.symbol.range.contains(position) {
        return None;
    }
    let row = position
        .line
        .checked_sub(card.source.symbol.range.start.line)? as usize;
    let text = card.source.code.lines().nth(row)?;
    let first_character = if row == 0 {
        card.source.symbol.range.start.character
    } else {
        0
    };
    if let Some(token) = card.source.tokens.iter().find(|token| {
        token.line == position.line
            && token.start <= position.character
            && token
                .start
                .checked_add(token.length)
                .is_some_and(|end| position.character < end)
    }) {
        let start = token.start.saturating_sub(first_character);
        let end = token
            .start
            .checked_add(token.length)?
            .saturating_sub(first_character);
        let start = refscape_model::utf16_byte_offset(text, start)?;
        let end = refscape_model::utf16_byte_offset(text, end)?;
        return (end > start).then_some((row, start..end));
    }
    let byte =
        refscape_model::utf16_byte_offset(text, position.character.checked_sub(first_character)?)?;
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    if !text[byte..].chars().next().is_some_and(is_word) {
        return None;
    }
    let start = text[..byte]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| is_word(*ch))
        .last()
        .map_or(byte, |(index, _)| index);
    let end = text[byte..]
        .char_indices()
        .take_while(|(_, ch)| is_word(*ch))
        .last()
        .map(|(index, ch)| byte + index + ch.len_utf8())?;
    Some((row, start..end))
}

fn code_connections(
    session: &Session,
    canvas: Bounds<Pixels>,
    window: &mut Window,
) -> Vec<CodeConnection> {
    if session.viewport.zoom < 0.65 {
        return vec![];
    }
    let zoom = session.viewport.zoom;
    session
        .connections
        .iter()
        .filter_map(|connection| {
            let from = session
                .cards
                .iter()
                .find(|card| card.id == connection.from)?;
            let to = session.cards.iter().find(|card| card.id == connection.to)?;
            let (row, span) = connected_word(from, connection.source)?;
            let text = from.source.code.lines().nth(row)?;
            let runs = code_runs(
                text,
                connection.source.line,
                if row == 0 {
                    from.source.symbol.range.start.character
                } else {
                    0
                },
                &from.source.tokens,
                &session.theme.palette,
            );
            let line = window.text_system().shape_line(
                text.to_string().into(),
                px(12.0 * zoom),
                &runs,
                None,
            );
            let rect = card_bounds(from, session, canvas);
            let x = rect.left() + px(54.0 * zoom);
            let y = rect.top()
                + px((HEADER + 8.0 + row as f32 * LINE) * zoom)
                + gpui::underline_y_offset(px(LINE * zoom), line.ascent, line.descent);
            let underline = Bounds::new(
                point(x + line.x_for_index(span.start), y),
                size(
                    line.x_for_index(span.end) - line.x_for_index(span.start),
                    px((1.5 * zoom).max(1.0)),
                ),
            );
            let target = card_bounds(to, session, canvas);
            let start = point(underline.right(), y + underline.size.height / 2.0);
            Some(CodeConnection {
                source_card: from.id.clone(),
                underline,
                start,
                exit: point(rect.right() + px(24.0 * zoom), start.y),
                end: point(target.left(), target.top() + px(HEADER * zoom * 0.5)),
            })
        })
        .collect()
}
fn paint_canvas(
    session: &Session,
    selected: Option<&str>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<PaintedCard> {
    let palette = &session.theme.palette;
    let zoom = session.viewport.zoom;
    let step = (32.0 * zoom).max(12.0);
    let mut y = session.viewport.offset.y.rem_euclid(step);
    while y < f32::from(bounds.size.height) {
        let mut x = session.viewport.offset.x.rem_euclid(step);
        while x < f32::from(bounds.size.width) {
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.left() + px(x), bounds.top() + px(y)),
                    size(px(1.0), px(1.0)),
                ),
                color(&palette.border),
            ));
            x += step;
        }
        y += step;
    }
    for region in &session.regions {
        let is_crate = region.id.starts_with("project:") || region.id.starts_with("crate:");
        if is_crate != (zoom < 0.35) {
            continue;
        }
        let rects: Vec<_> = session
            .cards
            .iter()
            .filter(|card| region.card_ids.contains(&card.id))
            .map(|card| card_bounds(card, session, bounds))
            .collect();
        if rects.is_empty() {
            continue;
        }
        let left = rects
            .iter()
            .map(|r| f32::from(r.left()))
            .fold(f32::INFINITY, f32::min)
            - 22.0 * zoom;
        let top = rects
            .iter()
            .map(|r| f32::from(r.top()))
            .fold(f32::INFINITY, f32::min)
            - 36.0 * zoom;
        let right = rects
            .iter()
            .map(|r| f32::from(r.right()))
            .fold(f32::NEG_INFINITY, f32::max)
            + 22.0 * zoom;
        let bottom = rects
            .iter()
            .map(|r| f32::from(r.bottom()))
            .fold(f32::NEG_INFINITY, f32::max)
            + 22.0 * zoom;
        window.paint_quad(quad(
            Bounds::new(
                point(px(left), px(top)),
                size(px(right - left), px(bottom - top)),
            ),
            px(10.0),
            color(&palette.surface_alt).opacity(0.35),
            px(1.0),
            color(&palette.border),
            Default::default(),
        ));
        text(
            region.label.clone(),
            point(px(left + 12.0), px(top + 8.0)),
            (13.0 * zoom).max(10.0),
            color(&palette.muted),
            window,
            cx,
        );
    }
    let connections = code_connections(session, bounds, window);
    for connection in &connections {
        let end = connection.end;
        let mut path = PathBuilder::stroke(connection.underline.size.height);
        path.move_to(connection.start);
        path.line_to(connection.exit);
        path.line_to(point(end.x - px(24.0 * zoom), end.y));
        path.line_to(end);
        if let Ok(path) = path.build() {
            window.paint_path(path, color(&palette.connection));
        }
        let mut arrow = PathBuilder::stroke(px(1.5));
        arrow.move_to(point(end.x - px(7.0), end.y - px(4.0)));
        arrow.line_to(end);
        arrow.line_to(point(end.x - px(7.0), end.y + px(4.0)));
        if let Ok(path) = arrow.build() {
            window.paint_path(path, color(&palette.connection));
        }
    }
    let mut painted = vec![];
    for card in &session.cards {
        let rect = card_bounds(card, session, bounds);
        if zoom < 0.35
            || rect.right() < bounds.left()
            || rect.left() > bounds.right()
            || rect.bottom() < bounds.top()
            || rect.top() > bounds.bottom()
        {
            continue;
        }
        window.paint_quad(quad(
            rect,
            px(7.0 * zoom),
            color(&palette.surface),
            px(if selected == Some(card.id.as_str()) {
                2.0
            } else {
                1.0
            }),
            color(if selected == Some(card.id.as_str()) {
                &palette.accent
            } else {
                &palette.border
            }),
            Default::default(),
        ));
        let (origin, lines) =
            window.with_content_mask(Some(gpui::ContentMask { bounds: rect }), |window| {
                text(
                    card.source.symbol.name.clone(),
                    point(rect.left() + px(16.0 * zoom), rect.top() + px(8.0 * zoom)),
                    (13.0 * zoom).max(9.0),
                    color(&palette.text),
                    window,
                    cx,
                );
                text(
                    card.source
                        .symbol
                        .path
                        .strip_prefix(&session.project_root)
                        .unwrap_or(&card.source.symbol.path)
                        .display()
                        .to_string(),
                    point(rect.left() + px(16.0 * zoom), rect.top() + px(29.0 * zoom)),
                    (10.0 * zoom).max(8.0),
                    color(&palette.muted),
                    window,
                    cx,
                );
                text(
                    "×".into(),
                    point(rect.right() - px(22.0 * zoom), rect.top() + px(8.0 * zoom)),
                    (15.0 * zoom).max(9.0),
                    color(&palette.muted),
                    window,
                    cx,
                );
                let origin = point(
                    rect.left() + px(54.0 * zoom),
                    rect.top() + px((HEADER + 8.0) * zoom),
                );
                let mut lines = vec![];
                if zoom >= 0.65 {
                    for (index, source) in card.source.code.lines().enumerate() {
                        let y = origin.y + px(index as f32 * LINE * zoom);
                        let runs = code_runs(
                            source,
                            card.source.symbol.range.start.line + index as u32,
                            if index == 0 {
                                card.source.symbol.range.start.character
                            } else {
                                0
                            },
                            &card.source.tokens,
                            palette,
                        );
                        let line = window.text_system().shape_line(
                            source.to_string().into(),
                            px(12.0 * zoom),
                            &runs,
                            None,
                        );
                        if y + px(LINE * zoom) >= bounds.top() && y < bounds.bottom() {
                            text(
                                format!(
                                    "{}",
                                    card.source.symbol.range.start.line + index as u32 + 1
                                ),
                                point(rect.left() + px(12.0 * zoom), y),
                                10.0 * zoom,
                                color(&palette.muted),
                                window,
                                cx,
                            );
                            let _ = line.paint(
                                point(origin.x, y),
                                px(LINE * zoom),
                                TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                        }
                        lines.push(line);
                    }
                } else {
                    text(
                        format!(
                            "{} · {} lines",
                            card.source.symbol.kind,
                            card.source.code.lines().count()
                        ),
                        origin,
                        11.0,
                        color(&palette.muted),
                        window,
                        cx,
                    );
                }
                for connection in connections
                    .iter()
                    .filter(|connection| connection.source_card == card.id)
                {
                    window.paint_quad(fill(connection.underline, color(&palette.connection)));
                    // Paint the part inside the card above its background; the outer
                    // route stays behind cards so unrelated source text is never crossed.
                    let mut path = PathBuilder::stroke(connection.underline.size.height);
                    path.move_to(connection.start);
                    path.line_to(connection.exit);
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color(&palette.connection));
                    }
                }
                (origin, lines)
            });
        painted.push(PaintedCard {
            id: card.id.clone(),
            bounds: rect,
            lines,
            first_line: card.source.symbol.range.start.line,
            first_character: card.source.symbol.range.start.character,
            origin,
        });
    }
    if session.cards.is_empty() {
        text(
            "Your code, connected.".into(),
            point(bounds.left() + px(80.0), bounds.top() + px(100.0)),
            30.0,
            color(&palette.text),
            window,
            cx,
        );
        text(
            "Open a project, then choose a file or search for a symbol.".into(),
            point(bounds.left() + px(80.0), bounds.top() + px(150.0)),
            14.0,
            color(&palette.muted),
            window,
            cx,
        );
        text(
            "Definitions and references unfold as cards on this canvas.".into(),
            point(bounds.left() + px(80.0), bounds.top() + px(180.0)),
            14.0,
            color(&palette.muted),
            window,
            cx,
        );
    }
    painted
}
fn code_runs(
    text: &str,
    line: u32,
    first_character: u32,
    tokens: &[refscape_model::SemanticToken],
    palette: &Palette,
) -> Vec<TextRun> {
    let mut runs = vec![];
    let mut cursor = 0;
    let font = gpui::font("Cascadia Code");
    let make = |len, value: &str| TextRun {
        len,
        font: font.clone(),
        color: color(value),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut tokens: Vec<_> = tokens.iter().filter(|token| token.line == line).collect();
    tokens.sort_by_key(|token| token.start);
    for token in tokens {
        let start =
            input::utf16_to_byte(text, token.start.saturating_sub(first_character) as usize);
        let end = input::utf16_to_byte(
            text,
            token
                .start
                .saturating_add(token.length)
                .saturating_sub(first_character) as usize,
        );
        if start < cursor || end <= start {
            continue;
        }
        if start > cursor {
            runs.push(make(start - cursor, &palette.text));
        }
        let value = match token.kind.as_str() {
            "keyword" | "modifier" => &palette.syntax_keyword,
            "string" | "number" | "regexp" => &palette.syntax_string,
            "type" | "struct" | "class" | "enum" | "interface" | "typeParameter" | "namespace" => {
                &palette.syntax_type
            }
            "function" | "method" | "macro" => &palette.syntax_function,
            "comment" => &palette.muted,
            _ => &palette.text,
        };
        runs.push(make(end - start, value));
        cursor = end;
    }
    if cursor < text.len() {
        runs.push(make(text.len() - cursor, &palette.text));
    }
    if runs.is_empty() {
        runs.push(make(text.len(), &palette.text));
    }
    runs
}
