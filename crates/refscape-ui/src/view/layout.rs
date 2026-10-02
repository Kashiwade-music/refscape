//! Layout actions carry intent; planning and commit belong to the controller.
use super::ExplorerView;
use gpui::Context;
use refscape_application::Command;
#[derive(Default)]
pub(super) struct LayoutState {
    pub(super) can_undo: bool,
}
impl ExplorerView {
    pub(super) fn layout_activity(&mut self, cx: &mut Context<Self>) {
        self.command(
            Command::Interaction {
                selected: self.canvas.selected.clone(),
                dragging: self.canvas.drag.is_some(),
            },
            cx,
        );
    }
    pub(super) fn arrange_layout(&mut self, cx: &mut Context<Self>) {
        self.command(
            Command::Arrange {
                selected: self.canvas.selected.clone(),
            },
            cx,
        );
    }
    pub(super) fn undo_layout(&mut self, cx: &mut Context<Self>) {
        self.command(Command::UndoLayout, cx);
    }
}
