//! Source navigation translates native input to application intent.
use super::ExplorerView;
use gpui::Context;
use refscape_application::Command;
use refscape_model::{Point, Symbol};
use std::path::PathBuf;
impl ExplorerView {
    pub(super) fn search(&mut self, cx: &mut Context<Self>) {
        self.command(Command::Search(self.search.query.clone()), cx);
    }
    pub(super) fn insertion_point(&self) -> Point {
        self.controller
            .snapshot()
            .viewport
            .screen_to_world(Point::new(100.0, 80.0))
    }
    pub(super) fn add_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.command(
            Command::AddFile {
                path,
                position: self.insertion_point(),
            },
            cx,
        );
    }
    pub(super) fn toggle_symbol(&mut self, symbol: Symbol, cx: &mut Context<Self>) {
        self.command(
            Command::AddSymbol {
                symbol,
                position: self.insertion_point(),
                toggle: true,
            },
            cx,
        );
    }
    pub(super) fn remove_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.canvas.selected.clone() {
            self.clear_inspection();
            self.command(Command::CloseCard { id }, cx);
        }
    }
    pub(super) fn cycle_theme(&mut self, cx: &mut Context<Self>) {
        let index = self
            .project
            .themes
            .iter()
            .position(|t| *t == self.controller.snapshot().theme)
            .unwrap_or(0);
        let theme = self.project.themes[(index + 1) % self.project.themes.len()].clone();
        self.command(Command::SetTheme(theme), cx);
    }
}
