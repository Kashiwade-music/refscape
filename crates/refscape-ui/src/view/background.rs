//! GPUI driver: effects own their inputs; the entity alone owns the controller.
use super::{CanvasState, ExplorerView, SearchState};
use gpui::{Context, Window};
use refscape_application::{Command, Transition, ViewEvent};
impl ExplorerView {
    pub(super) fn command(&mut self, command: Command, cx: &mut Context<Self>) {
        let transition = self.controller.dispatch(command);
        self.transition(transition, cx);
    }
    pub(super) fn transition(&mut self, transition: Transition, cx: &mut Context<Self>) {
        for event in transition.events {
            match event {
                ViewEvent::Reset => {
                    self.project.launch_options = Default::default();
                    self.canvas = CanvasState::default();
                    self.search = SearchState::default();
                    self.clear_hover(cx);
                    self.scene.borrow_mut().clear();
                }
                ViewEvent::Files(files) => {
                    self.sidebar.catalog(&files);
                    self.project.files = files;
                }
                ViewEvent::Symbols(symbols) => self.project.symbols = symbols,
                ViewEvent::ClearInspection => self.canvas.inspection = None,
                ViewEvent::Inspection(inspection) => self.canvas.inspection = Some(inspection),
                ViewEvent::Hover(text) => {
                    self.hover.text = text.filter(|text| !text.trim().is_empty())
                }
                ViewEvent::Status { message, error } => {
                    self.requests.status = message;
                    self.requests.error = error;
                }
                ViewEvent::Canvas(outcome) => {
                    if let Some(target) = outcome.targets.first() {
                        self.canvas.selected = Some(target.clone());
                    } else if outcome.expanded == Some(false) {
                        self.canvas.selected = outcome.origin;
                    }
                    self.reconcile_canvas_selection();
                }
                ViewEvent::CloseWindow => self.close_ready = true,
            }
        }
        for effect in transition.effects {
            let executor = self.executor.clone();
            let task = cx
                .background_executor()
                .spawn(async move { executor.execute(effect) });
            cx.spawn(async move |view, cx| {
                let completion = task.await;
                let _ = view.update(cx, |view, cx| {
                    let transition = view.controller.complete(completion);
                    view.transition(transition, cx);
                });
            })
            .detach();
        }
        self.requests.busy = self.controller.busy();
        self.requests.closing = self.controller.closing();
        self.layout.can_undo = self.controller.can_undo_layout();
        if let Some(path) = self.controller.destination().path() {
            self.project.session_path = path.to_path_buf();
        }
        let theme = &self.controller.snapshot().theme;
        if !self.project.themes.contains(theme) {
            self.project.themes.push(theme.clone());
        }
        cx.notify();
    }
    pub(super) fn close(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_ready {
            return true;
        }
        self.command(Command::RequestClose, cx);
        self.close_ready
    }
}
