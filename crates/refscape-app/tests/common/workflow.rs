//! Controller-driven workflow fixture. All mutations enter through public commands.
#![allow(dead_code)]
use refscape_application::{
    ApplicationSnapshot, Command, HeadlessDriver, VariableInspection, ViewEvent, WorkerExecutor,
};
use refscape_language::LanguageBackend;
use refscape_model::{
    CODE_CARD_HEADER, CODE_LINE_HEIGHT, ConnectionKind, Point, Position, ProjectOpenOptions,
    Symbol, Theme,
};
use refscape_storage::session::JsonSessionRepository;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
pub struct Workflow {
    driver: HeadlessDriver,
}
impl Workflow {
    pub fn new(factory: LanguageBackend, repository: JsonSessionRepository) -> Self {
        Self {
            driver: HeadlessDriver::new(Arc::new(WorkerExecutor::new(
                Arc::new(factory),
                Arc::new(repository),
            ))),
        }
    }
    fn run(&mut self, command: Command) -> Result<Vec<ViewEvent>, String> {
        let events = self.driver.dispatch(command);
        if let Some(message) = events.iter().find_map(|event| match event {
            ViewEvent::Status {
                message,
                error: true,
            } => Some(message.clone()),
            _ => None,
        }) {
            return Err(message);
        }
        Ok(events)
    }
    pub fn session(&self) -> &ApplicationSnapshot {
        self.driver.controller.snapshot()
    }
    pub fn open_project(
        &mut self,
        root: &Path,
        options: &ProjectOpenOptions,
    ) -> Result<(), String> {
        self.run(Command::OpenProject {
            root: root.to_path_buf(),
            options: options.clone(),
            destination: root.join(".refscape/workflow.json"),
        })?;
        Ok(())
    }
    pub fn load_session(&mut self, path: &Path) -> Result<(), String> {
        self.run(Command::OpenSession {
            path: path.to_path_buf(),
            expected_root: None,
            overrides: ProjectOpenOptions::default(),
        })?;
        Ok(())
    }
    pub fn load_project_session(
        &mut self,
        path: &Path,
        root: &Path,
        options: &ProjectOpenOptions,
    ) -> Result<(), String> {
        self.run(Command::OpenSession {
            path: path.to_path_buf(),
            expected_root: Some(root.to_path_buf()),
            overrides: options.clone(),
        })?;
        Ok(())
    }
    pub fn save_session(&mut self, path: &Path) -> Result<(), String> {
        self.run(Command::SaveAs(path.to_path_buf()))?;
        Ok(())
    }
    pub fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        Ok(self
            .run(Command::Files)?
            .into_iter()
            .find_map(|event| match event {
                ViewEvent::Files(files) => Some(files),
                _ => None,
            })
            .expect("Files result"))
    }
    pub fn refresh_sources(&mut self) -> Result<(), String> {
        self.run(Command::RefreshSources)?;
        Ok(())
    }
    pub fn symbols(&mut self, path: &Path) -> Result<Vec<Symbol>, String> {
        Ok(self
            .run(Command::Symbols(path.to_path_buf()))?
            .into_iter()
            .find_map(|event| match event {
                ViewEvent::Symbols(symbols) => Some(symbols),
                _ => None,
            })
            .expect("Symbols result"))
    }
    pub fn search(&mut self, query: &str) -> Result<Vec<Symbol>, String> {
        Ok(self
            .run(Command::Search(query.into()))?
            .into_iter()
            .find_map(|event| match event {
                ViewEvent::Symbols(symbols) => Some(symbols),
                _ => None,
            })
            .expect("Search result"))
    }
    fn targets(events: Vec<ViewEvent>) -> Vec<String> {
        events
            .into_iter()
            .filter_map(|event| match event {
                ViewEvent::Canvas(outcome) => Some(outcome.targets),
                _ => None,
            })
            .flatten()
            .collect()
    }
    pub fn add_symbol(&mut self, symbol: Symbol, position: Point) -> Result<String, String> {
        Ok(Self::targets(self.run(Command::AddSymbol {
            symbol,
            position,
            toggle: false,
        })?)
        .remove(0))
    }
    pub fn add_file(&mut self, path: &Path, position: Point) -> Result<String, String> {
        Ok(Self::targets(self.run(Command::AddFile {
            path: path.to_path_buf(),
            position,
        })?)
        .remove(0))
    }
    fn navigate(
        &mut self,
        card: &str,
        position: Position,
        kind: ConnectionKind,
        toggle: bool,
    ) -> Result<(Vec<String>, bool), String> {
        let source = self
            .session()
            .cards
            .iter()
            .find(|current| current.id == card)
            .ok_or("Unknown card")?;
        let anchor = Point::new(
            source.width,
            CODE_CARD_HEADER
                + 8.0
                + source.source.display_anchor_row(position).unwrap_or(0) as f32 * CODE_LINE_HEIGHT,
        );
        let events = self.run(Command::Navigate {
            card: card.into(),
            position,
            kind,
            anchor,
            toggle,
        })?;
        let expanded = events.iter().any(
            |event| matches!(event,ViewEvent::Canvas(outcome) if outcome.expanded==Some(true)),
        );
        Ok((Self::targets(events), expanded))
    }
    pub fn expand_definition(
        &mut self,
        card: &str,
        position: Position,
    ) -> Result<Vec<String>, String> {
        Ok(self
            .navigate(card, position, ConnectionKind::Definition, false)?
            .0)
    }
    pub fn expand_references(
        &mut self,
        card: &str,
        position: Position,
    ) -> Result<Vec<String>, String> {
        Ok(self
            .navigate(card, position, ConnectionKind::Reference, false)?
            .0)
    }
    pub fn toggle_type_definition(
        &mut self,
        card: &str,
        position: Position,
    ) -> Result<Option<Vec<String>>, String> {
        let (targets, expanded) =
            self.navigate(card, position, ConnectionKind::TypeDefinition, true)?;
        Ok(expanded.then_some(targets))
    }
    pub fn inspect_variable(
        &mut self,
        card: &str,
        position: Position,
    ) -> Result<Option<VariableInspection>, String> {
        Ok(self
            .run(Command::Inspect {
                card: card.into(),
                position,
            })?
            .into_iter()
            .find_map(|event| match event {
                ViewEvent::Inspection(value) => Some(value),
                _ => None,
            }))
    }
    pub fn hover(&mut self, card: &str, position: Position) -> Result<Option<String>, String> {
        Ok(self
            .run(Command::Hover {
                card: card.into(),
                position,
            })?
            .into_iter()
            .find_map(|event| match event {
                ViewEvent::Hover(value) => value,
                _ => None,
            }))
    }
    pub fn move_card(&mut self, id: &str, position: Point) -> Result<(), String> {
        self.run(Command::MoveCard {
            id: id.into(),
            position,
        })?;
        Ok(())
    }
    pub fn remove_card(&mut self, id: &str) -> Result<(), String> {
        self.run(Command::CloseCard { id: id.into() })?;
        Ok(())
    }
    pub fn expand_context(&mut self, id: &str, index: usize) -> Result<(), String> {
        self.run(Command::ToggleFold {
            card: id.into(),
            index,
            expand: true,
        })?;
        Ok(())
    }
    pub fn collapse_context(&mut self, id: &str, index: usize) -> Result<(), String> {
        self.run(Command::ToggleFold {
            card: id.into(),
            index,
            expand: false,
        })?;
        Ok(())
    }
    pub fn arrange_layout(&mut self, selected: Option<&str>) -> Result<bool, String> {
        let before = self.session().clone();
        self.run(Command::Arrange {
            selected: selected.map(str::to_owned),
        })?;
        Ok(self.session().cards != before.cards)
    }
    pub fn undo_layout(&mut self) -> Result<bool, String> {
        let available = self.driver.controller.can_undo_layout();
        self.run(Command::UndoLayout)?;
        Ok(available)
    }
    pub fn pan(&mut self, delta: Point) -> Result<(), String> {
        self.run(Command::Pan(delta))?;
        Ok(())
    }
    pub fn zoom(&mut self, factor: f32, anchor: Point) -> Result<(), String> {
        self.run(Command::Zoom { factor, anchor })?;
        Ok(())
    }
    pub fn set_theme(&mut self, theme: Theme) -> Result<(), String> {
        self.run(Command::SetTheme(theme))?;
        Ok(())
    }
}
impl Drop for Workflow {
    fn drop(&mut self) {
        let epoch = self.driver.controller.basis().project;
        let _ = self
            .driver
            .executor
            .execute(refscape_application::Effect::DisposeProject { epoch });
    }
}
