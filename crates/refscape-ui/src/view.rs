//! View state and initialization. UI operations live in cohesive child modules.
mod background;
mod hover;
mod input;
mod interaction;
mod layout;
mod navigation;
mod painting;
mod project;
mod render;
mod shaping;
#[cfg(test)]
mod tests;

use background::Output;
use interaction::Drag;
use painting::{PaintedCard, card_bounds, card_height, paint_canvas};
use render::{color, display_path};
use shaping::{code_connections, connected_word};

use gpui::{
    App, Bounds, Context, ElementInputHandler, FocusHandle, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, PathBuilder, PathPromptOptions, Pixels, Render, ScrollDelta,
    ScrollWheelEvent, ShapedLine, TextAlign, TextRun, Window, canvas, div, fill, point, prelude::*,
    px, quad, rgb, size,
};
use refscape_application::{
    explorer::{
        Explorer, PreparedCanvasCommit, PreparedCanvasEdit, PreparedLayoutCommit,
        VariableInspection,
    },
    ports::{LanguageService, SessionRepository},
};
use refscape_model::{
    CODE_CARD_HEADER, CODE_LINE_HEIGHT, CODE_REGION_HEADER, CODE_REGION_PADDING, CodeCard,
    ConnectionKind, Palette, Point, Position, ProjectLanguage, ProjectOptions, Session, Symbol,
    Theme,
};
use std::{
    ops::Range,
    path::PathBuf,
    sync::{Arc, Mutex},
};

const HEADER: f32 = CODE_CARD_HEADER;
const LINE: f32 = CODE_LINE_HEIGHT;

/// GPUI view constructed by the composition root with concrete adapters.
pub(crate) struct ExplorerView<L: LanguageService + 'static, R: SessionRepository + 'static> {
    explorer: Arc<Mutex<Explorer<L, R>>>,
    session: Session,
    focus: FocusHandle,
    project: ProjectState,
    search: SearchState,
    pub(crate) canvas: CanvasState,
    hover: HoverState,
    requests: RequestState,
    layout: layout::LayoutState,
}

struct ProjectState {
    session_path: PathBuf,
    themes: Vec<Theme>,
    files: Vec<PathBuf>,
    symbols: Vec<Symbol>,
    autosave: bool,
    pending: Option<(PathBuf, PathBuf)>,
    launch_options: ProjectOptions,
}

#[derive(Default)]
struct SearchState {
    query: String,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
    bounds: Option<Bounds<Pixels>>,
    line: Option<ShapedLine>,
    focused: bool,
}

#[derive(Default)]
pub(crate) struct CanvasState {
    pub(crate) selected: Option<String>,
    drag: Option<Drag>,
    painted: Vec<PaintedCard>,
    pub(crate) inspection: Option<VariableInspection>,
    selection_generation: u64,
    bounds: Bounds<Pixels>,
    context_hover: Option<(String, usize)>,
    drag_preview: Option<(String, Point)>,
}

#[derive(Default)]
struct HoverState {
    target: Option<hover::HoverTarget>,
    text: Option<String>,
    task: Option<gpui::Task<()>>,
    dismiss_task: Option<gpui::Task<()>>,
    pending_target: Option<hover::HoverTarget>,
    scroll: gpui::ScrollHandle,
}

#[derive(Default)]
struct RequestState {
    busy: bool,
    status: String,
    error: bool,
    closing: bool,
}

impl<L: LanguageService + 'static, R: SessionRepository + 'static> ExplorerView<L, R> {
    /// Indexing and initial session restoration begin after the window is created.
    pub(crate) fn new(
        explorer: Explorer<L, R>,
        session_path: PathBuf,
        custom_themes: Vec<Theme>,
        initial_project: Option<PathBuf>,
        project_options: ProjectOptions,
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
            focus,
            project: ProjectState {
                session_path,
                themes,
                files: Vec::new(),
                symbols: Vec::new(),
                autosave: true,
                pending: None,
                launch_options: project_options.clone(),
            },
            search: SearchState::default(),
            canvas: CanvasState::default(),
            hover: HoverState::default(),
            requests: RequestState {
                status: "Open a project to start exploring.".into(),
                ..Default::default()
            },
            layout: layout::LayoutState::default(),
        };
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |view, cx| view.close(window, cx))
                .unwrap_or(true)
        });
        if let Some(path) = initial_project {
            let session_path = if view.project.session_path.as_os_str().is_empty() {
                path.join(".refscape/session.json")
            } else {
                view.project.session_path.clone()
            };
            view.open_project_with_session_options(path, session_path, project_options, cx);
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
}
