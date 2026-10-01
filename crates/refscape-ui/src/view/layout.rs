//! Placement staging and explicit layout controls.
use super::*;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Default)]
pub(super) struct LayoutState {
    pub(super) revision: u64,
    pub(super) session_epoch: u64,
    pub(super) pending_output: Option<Output>,
    pub(super) pending_drop: Option<(String, Point)>,
    pub(super) planning: bool,
    pub(super) backend_pending: bool,
    pub(super) explicit_pending: bool,
    pub(super) can_undo: bool,
    cancel: Option<Arc<AtomicBool>>,
}

#[derive(Clone)]
pub(super) enum Placement {
    Edit(Box<PreparedCanvasEdit>),
    Drop(String, Point),
}

pub(super) fn capture_layout_snapshot<L: LanguageService, R: SessionRepository>(
    explorer: &Arc<Mutex<Explorer<L, R>>>,
    cancel: &AtomicBool,
) -> Result<refscape_application::explorer::CanvasLayoutSnapshot, String> {
    let explorer = explorer
        .lock()
        .map_err(|_| "Explorer lock poisoned".to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Layout calculation cancelled".into());
    }
    // Confirmed geometry lives in Explorer; camera input cannot alter packing.
    Ok(explorer.layout_snapshot())
}

impl<L: LanguageService + 'static, R: SessionRepository + 'static> ExplorerView<L, R> {
    /// New input invalidates an in-flight explicit layout, including camera-only input.
    pub(super) fn layout_activity(&mut self, _cx: &mut Context<Self>) {
        self.layout.revision = self.layout.revision.wrapping_add(1);
        if let Some(cancel) = self.layout.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn reset_canvas_layout(&mut self) {
        if let Some(cancel) = self.layout.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.layout = LayoutState {
            session_epoch: self.layout.session_epoch.wrapping_add(1),
            revision: self.layout.revision.wrapping_add(1),
            ..Default::default()
        };
    }

    /// A backend result remains busy until a drop and placement have both committed.
    pub(super) fn resume_canvas_edit(&mut self, cx: &mut Context<Self>) {
        if self.layout.planning
            || self.layout.backend_pending
            || self.canvas.drag.is_some()
            || self.requests.closing
        {
            return;
        }
        let placement = if let Some((id, position)) = self.layout.pending_drop.take() {
            Some(Placement::Drop(id, position))
        } else {
            self.layout
                .pending_output
                .as_ref()
                .and_then(|output| output.prepared.clone())
                .map(Box::new)
                .map(Placement::Edit)
        };
        if let Some(placement) = placement {
            self.plan_placement(placement, cx);
        } else if self.layout.explicit_pending {
            self.layout.explicit_pending = false;
            self.start_layout(cx);
        }
    }

    fn plan_placement(&mut self, placement: Placement, cx: &mut Context<Self>) {
        self.requests.busy = true;
        self.layout.planning = true;
        let epoch = self.layout.session_epoch;
        let revision = self.layout.revision;
        let explorer = self.explorer.clone();
        let viewport = self.session.viewport;
        let positions = self
            .session
            .cards
            .iter()
            .map(|card| (card.id.clone(), card.position))
            .collect();
        let retry = placement.clone();
        let task = cx.background_executor().spawn(async move {
            let mut explorer = explorer
                .lock()
                .map_err(|_| "Explorer lock poisoned".to_string())?;
            explorer.sync_canvas(viewport, positions)?;
            let edit = match placement {
                Placement::Edit(edit) => *edit,
                Placement::Drop(id, position) => explorer.prepare_move_card(&id, position)?,
            };
            explorer.plan_prepared(edit)
        });
        cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                if epoch != view.layout.session_epoch {
                    return;
                }
                view.layout.planning = false;
                if revision != view.layout.revision {
                    // Reuse resolved source. No second language-server request is made.
                    if let Placement::Drop(id, position) = retry
                        && view.layout.pending_drop.is_none()
                    {
                        view.layout.pending_drop = Some((id, position));
                    }
                    view.requests.busy = view.layout.pending_output.is_some();
                    view.resume_canvas_edit(cx);
                    return;
                }
                view.complete_placement(result, retry, cx);
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn complete_placement(
        &mut self,
        result: Result<PreparedCanvasCommit, String>,
        placement: Placement,
        cx: &mut Context<Self>,
    ) {
        let result = match result {
            Ok(commit) => {
                let explorer = self.explorer.clone();
                match explorer.try_lock() {
                    Ok(mut explorer) => explorer.apply_commit(commit).map(|outcome| {
                        (
                            outcome,
                            explorer.session().clone(),
                            explorer.can_undo_layout(),
                        )
                    }),
                    Err(std::sync::TryLockError::Poisoned(_)) => {
                        Err("Explorer lock poisoned".into())
                    }
                    Err(std::sync::TryLockError::WouldBlock) => {
                        self.defer_placement(commit, placement, cx);
                        return;
                    }
                }
            }
            Err(error) => Err(error),
        };
        match result {
            Ok((outcome, mut session, can_undo)) => {
                session.viewport = self.session.viewport;
                self.session = session;
                self.layout.can_undo = can_undo;
                if let Some(id) = outcome
                    .targets
                    .iter()
                    .find(|id| !outcome.added.contains(id))
                {
                    self.canvas.selected = Some(id.clone());
                }
                if matches!(placement, Placement::Edit(_)) {
                    let mut output = self.layout.pending_output.take().unwrap_or_default();
                    output.prepared = None;
                    output.can_undo = can_undo;
                    if outcome.expanded == Some(true) && outcome.targets.is_empty() {
                        output.message = Some(if output.inspection.is_some() {
                            "Variable highlighted; this type has no source definition to expand."
                                .into()
                        } else {
                            "The language service returned no locations for this position.".into()
                        });
                    }
                    self.complete_ready_output(output, cx);
                }
            }
            Err(error) => {
                self.layout.pending_output = None;
                self.requests.status = error;
                self.requests.error = true;
            }
        }
        self.requests.busy = false;
        self.reconcile_canvas_selection();
        self.resume_canvas_edit(cx);
        cx.notify();
    }

    fn defer_placement(
        &mut self,
        commit: PreparedCanvasCommit,
        placement: Placement,
        cx: &mut Context<Self>,
    ) {
        let epoch = self.layout.session_epoch;
        let revision = self.layout.revision;
        self.layout.planning = true;
        cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(10))
                .await;
            let _ = view.update(cx, |view, cx| {
                if epoch != view.layout.session_epoch {
                    return;
                }
                view.layout.planning = false;
                if revision != view.layout.revision {
                    if let Placement::Drop(id, position) = placement
                        && view.layout.pending_drop.is_none()
                    {
                        view.layout.pending_drop = Some((id, position));
                    }
                    view.requests.busy = view.layout.pending_output.is_some();
                    view.resume_canvas_edit(cx);
                } else {
                    view.complete_placement(Ok(commit), placement, cx);
                }
            });
        })
        .detach();
    }

    fn complete_ready_output(&mut self, output: Output, cx: &mut Context<Self>) {
        // Preserve metadata from preparation after the validated canvas was applied.
        if let Some((generation, inspection)) = output.inspection
            && generation == self.canvas.selection_generation
        {
            self.canvas.inspection = Some(inspection);
        }
        if let Some(symbols) = output.symbols {
            self.project.symbols = symbols;
        }
        if let Some(files) = output.files {
            self.project.files = files;
        }
        self.requests.error = output.error;
        self.requests.status = output.message.unwrap_or_else(|| {
            format!(
                "{} cards · {} connections · Ready",
                self.session.cards.len(),
                self.session.connections.len()
            )
        });
        cx.notify();
    }

    pub(super) fn arrange_layout(&mut self, cx: &mut Context<Self>) {
        if self.requests.busy || self.requests.closing || self.canvas.drag.is_some() {
            return;
        }
        self.layout_activity(cx);
        self.start_layout(cx);
    }

    fn start_layout(&mut self, cx: &mut Context<Self>) {
        let explorer = self.explorer.clone();
        let selected = self.canvas.selected.clone();
        let revision = self.layout.revision;
        let epoch = self.layout.session_epoch;
        self.requests.busy = true;
        self.layout.planning = true;
        self.requests.status = "Arranging layout…".into();
        self.requests.error = false;
        let cancel = Arc::new(AtomicBool::new(false));
        self.layout.cancel = Some(cancel.clone());
        let task = cx.background_executor().spawn(async move {
            let staged = capture_layout_snapshot(&explorer, &cancel)?;
            staged.plan_cancellable(selected.as_deref(), &|| cancel.load(Ordering::Relaxed))
        });
        let waiter = cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                view.complete_layout(result, epoch, revision, cx);
            });
        });
        waiter.detach();
        cx.notify();
    }

    pub(super) fn complete_layout(
        &mut self,
        result: Result<PreparedLayoutCommit, String>,
        epoch: u64,
        revision: u64,
        cx: &mut Context<Self>,
    ) {
        if epoch != self.layout.session_epoch {
            return;
        }
        if revision != self.layout.revision {
            self.layout.planning = false;
            self.layout.explicit_pending = true;
            self.resume_canvas_edit(cx);
            return;
        }
        self.layout.cancel = None;
        let result = match result {
            Ok(commit) => {
                let explorer = self.explorer.clone();
                match explorer.try_lock() {
                    Ok(mut explorer) => explorer.apply_layout_commit(commit).map(|changed| {
                        (
                            changed,
                            explorer.session().clone(),
                            explorer.can_undo_layout(),
                        )
                    }),
                    Err(std::sync::TryLockError::Poisoned(_)) => {
                        Err("Explorer lock poisoned".into())
                    }
                    Err(std::sync::TryLockError::WouldBlock) => {
                        cx.spawn(async move |view, cx| {
                            cx.background_executor()
                                .timer(Duration::from_millis(10))
                                .await;
                            let _ = view.update(cx, |view, cx| {
                                view.complete_layout(Ok(commit), epoch, revision, cx)
                            });
                        })
                        .detach();
                        return;
                    }
                }
            }
            Err(error) => Err(error),
        };
        match result {
            Ok((changed, mut session, can_undo)) => {
                session.viewport = self.session.viewport;
                self.session = session;
                self.layout.can_undo = can_undo;
                if changed {
                    self.requests.status =
                        "Layout arranged. Undo layout restores the previous positions.".into();
                    self.requests.error = false;
                } else {
                    self.requests.status = "Layout unchanged.".into();
                }
            }
            Err(error) => {
                self.requests.status = error;
                self.requests.error = true;
            }
        }
        self.requests.busy = false;
        self.layout.planning = false;
        self.resume_canvas_edit(cx);
        cx.notify();
    }

    pub(super) fn undo_layout(&mut self, cx: &mut Context<Self>) {
        if self.requests.busy || !self.layout.can_undo || self.canvas.drag.is_some() {
            return;
        }
        self.run_job(
            "Restoring layout…",
            Box::new(|explorer| {
                explorer.undo_layout()?;
                Ok(Output {
                    message: Some("Previous layout restored.".into()),
                    ..Default::default()
                })
            }),
            cx,
        );
    }
}
