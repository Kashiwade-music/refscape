//! Background jobs and reconciliation of results with ongoing UI interactions.
use super::*;

pub(super) type Job<L, R> = Box<dyn FnOnce(&mut Explorer<L, R>) -> Result<Output, String> + Send>;
#[derive(Default)]
pub(super) struct Output {
    pub(super) files: Option<Vec<PathBuf>>,
    pub(super) symbols: Option<Vec<Symbol>>,
    pub(super) reset: bool,
    pub(super) message: Option<String>,
    pub(super) session_path: Option<PathBuf>,
    pub(super) protect_session: bool,
    pub(super) inspection: Option<(u64, VariableInspection)>,
    pub(super) error: bool,
    pub(super) opened_project: bool,
    pub(super) restore_session: Option<(PathBuf, PathBuf)>,
    pub(super) prepared: Option<PreparedCanvasEdit>,
    pub(super) can_undo: bool,
}

impl<L: LanguageService + 'static, R: SessionRepository + 'static> ExplorerView<L, R> {
    pub(super) fn run_job(&mut self, label: &str, job: Job<L, R>, cx: &mut Context<Self>) {
        if self.requests.closing {
            return;
        }
        if self.requests.busy {
            self.requests.status = "A request is running. Try again when it finishes.".into();
            cx.notify();
            return;
        }
        if self.canvas.drag.is_some() {
            self.requests.status = "Finish dragging before starting another request.".into();
            cx.notify();
            return;
        }
        self.requests.busy = true;
        self.layout.backend_pending = true;
        self.layout_activity(cx);
        self.clear_hover(cx);
        self.requests.error = false;
        self.requests.status = label.into();
        let explorer = self.explorer.clone();
        let viewport = self.session.viewport;
        let positions: Vec<_> = self
            .session
            .cards
            .iter()
            .map(|c| (c.id.clone(), c.position))
            .collect();
        let epoch = self.layout.session_epoch;
        let task = cx.background_executor().spawn(async move {
            let mut explorer = explorer
                .lock()
                .map_err(|_| "Explorer lock poisoned".to_string())?;
            explorer.sync_canvas(viewport, positions)?;
            let result = job(&mut explorer).map(|mut output| {
                output.can_undo = explorer.can_undo_layout();
                output
            });
            Ok::<_, String>((result, explorer.session().clone()))
        });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                if epoch == view.layout.session_epoch {
                    view.complete_job(result, cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Reconcile a completed request while retaining newer pointer and selection state.
    pub(super) fn complete_job(
        &mut self,
        result: Result<(Result<Output, String>, Session), String>,
        cx: &mut Context<Self>,
    ) {
        self.layout.backend_pending = false;
        self.requests.busy = false;
        match result {
            Ok((Ok(output), _)) if output.prepared.is_some() => {
                self.requests.busy = true;
                self.layout.pending_output = Some(output);
                self.resume_canvas_edit(cx);
            }
            Ok((Ok(output), mut session)) => {
                if !output.reset {
                    session.viewport = self.session.viewport;
                }
                self.session = session;
                self.layout.can_undo = output.can_undo;
                if output.reset {
                    self.reset_canvas_layout();
                }
                if let Some((generation, inspection)) = output.inspection
                    && generation == self.canvas.selection_generation
                {
                    self.canvas.inspection = Some(inspection);
                }
                if !self.project.themes.contains(&self.session.theme) {
                    self.project.themes.push(self.session.theme.clone());
                }
                if let Some(files) = output.files {
                    self.project.files = files;
                }
                if let Some(symbols) = output.symbols {
                    self.project.symbols = symbols;
                }
                if let Some(path) = output.session_path {
                    self.project.session_path = path;
                    self.project.autosave = !output.protect_session;
                }
                if output.reset {
                    self.clear_inspection();
                    self.project.symbols.clear();
                    self.search.query.clear();
                    self.search.selection = 0..0;
                    self.search.marked = None;
                    self.canvas.selected = None;
                    self.canvas.drag = None;
                    self.canvas.drag_preview = None;
                }
                self.requests.status = output.message.unwrap_or_else(|| {
                    format!(
                        "{} cards · {} connections · Ready",
                        self.session.cards.len(),
                        self.session.connections.len()
                    )
                });
                self.requests.error = output.error;
                if output.opened_project {
                    self.project.pending = None;
                    self.project.launch_options = ProjectOptions::default();
                }
                self.reconcile_canvas_selection();
                if let Some((root, path)) = output.restore_session {
                    self.project.pending = Some((root.clone(), path.clone()));
                    self.restore_session(root, path, cx);
                }
            }
            Ok((Err(error), _)) => {
                self.requests.status = error;
                self.requests.error = true;
            }
            Err(error) => {
                self.requests.status = error;
                self.requests.error = true;
            }
        }
        if !self.requests.busy {
            self.resume_canvas_edit(cx);
        }
        cx.notify();
    }

    pub(super) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.canvas.drag.is_some() || self.layout.pending_drop.is_some() {
            self.requests.status = "Finish moving the canvas before closing.".into();
            cx.notify();
            return false;
        }
        if self.requests.busy {
            self.requests.status = "A request is running. Close again after it finishes so the complete session can be saved.".into();
            cx.notify();
            return false;
        }
        if self.session.project_root.as_os_str().is_empty() || !self.project.autosave {
            return true;
        }
        if self.requests.closing {
            return false;
        }
        self.requests.closing = true;
        self.layout_activity(cx);
        self.clear_hover(cx);
        self.requests.status = "Saving session before closing…".into();
        self.requests.error = false;
        let explorer = self.explorer.clone();
        let path = self.project.session_path.clone();
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
                        view.requests.closing = false;
                        view.requests.status = format!(
                            "Session save failed: {error}. Use Save as to choose another location."
                        );
                        view.requests.error = true;
                        cx.notify();
                    });
                }
            });
        })
        .detach();
        cx.notify();
        false
    }
}
